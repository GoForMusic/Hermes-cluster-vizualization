//! Changing the agent version of a source from the dashboard. The hub does not touch the cluster: it tells the agents, over the stream
//! they keep open, which version to run, and each agent changes its own image (see `agentkit::upgrade`). This remembers which agents are
//! connected and able to, what was asked for and how it is going, and tells "done" from "failed" by what the agents report.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hermes_proto::v1::{HubMessage, Upgrade, UpgradeState, UpgradeStatus, hub_message};
use tokio::sync::mpsc::Sender;
use tonic::Status;
use tracing::info;

use crate::model::UpgradeView;
use crate::services::AgentInfo;

/// An upgrade that has not finished by now did not work (the agents' own rollouts have shorter limits and say so themselves).
const GIVE_UP_AFTER: Duration = Duration::from_secs(600);
/// "Done" is news, not a state: it is shown for this long after the last agent reported the new version, then the card is quiet again.
/// A failure stays until the next change is asked for, because someone has to look at it.
const DONE_SHOWN_FOR: Duration = Duration::from_secs(600);
/// An agent that has not been heard of for this long is not counted: a replaced instance lingers in the list for a while.
const LIVE_WITHIN: Duration = Duration::from_secs(30);

pub type ToAgent = Sender<Result<HubMessage, Status>>;

#[derive(Debug, PartialEq, Eq)]
pub enum UpgradeError {
    /// No connected agent of the source was installed to upgrade itself.
    NoCapableAgent,
    /// The version is not one.
    BadVersion,
}

pub trait IUpgradeService: Send + Sync {
    /// An agent's session opened. `features` are what its hello said it can do. Returns a token for `detach`.
    fn attach(&self, source_id: &str, agent: &str, features: &[String], to: ToAgent) -> u64;
    /// The session ended. A newer session of the same agent (it reconnected) is left alone.
    fn detach(&self, source_id: &str, agent: &str, session: u64);
    /// Some connected agent of the source can change its own image.
    fn can_upgrade(&self, source_id: &str) -> bool;
    /// Asks every capable agent of the source to run `version`; how many were asked. Only one of them acts (a worker skips).
    fn request(&self, source_id: &str, version: &str) -> Result<usize, UpgradeError>;
    /// What an agent said about the attempt.
    fn report(&self, source_id: &str, agent: &str, status: &UpgradeStatus);
    /// How the last upgrade of the source is going, judged by what its agents run now.
    fn view(&self, source_id: &str, agents: &[AgentInfo]) -> Option<UpgradeView>;
    /// The source is gone.
    fn forget(&self, source_id: &str);
}

struct Session {
    id: u64,
    upgrade: bool,
    to: ToAgent,
}

struct Asked {
    version: String,
    at: Instant,
    failure: Option<String>,
    /// When every agent was first seen on the new version.
    finished: Option<Instant>,
}

#[derive(Default)]
struct State {
    next: u64,
    sessions: HashMap<String, HashMap<String, Session>>,
    asked: HashMap<String, Asked>,
}

#[derive(Default)]
pub struct UpgradeServiceImp {
    state: Mutex<State>,
}

impl UpgradeServiceImp {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn view_at(&self, now: Instant, source_id: &str, agents: &[AgentInfo]) -> Option<UpgradeView> {
        let mut state = self.lock();
        let asked = state.asked.get_mut(source_id)?;
        let version = asked.version.clone();
        let view = |state: &str, message: String| UpgradeView {
            version: version.clone(),
            state: state.into(),
            message,
        };
        if let Some(why) = &asked.failure {
            return Some(view("failed", why.clone()));
        }
        let live: Vec<&AgentInfo> = agents
            .iter()
            .filter(|a| a.seen_ago <= LIVE_WITHIN)
            .collect();
        let behind: Vec<&str> = live
            .iter()
            .filter(|a| crate::version::parse(&a.version) != crate::version::parse(&version))
            .map(|a| a.version.as_str())
            .collect();
        if !live.is_empty() && behind.is_empty() {
            let finished = *asked.finished.get_or_insert(now);
            return (now.saturating_duration_since(finished) <= DONE_SHOWN_FOR)
                .then(|| view("done", String::new()));
        }
        asked.finished = None; // one instance went back (a rollback): the count starts again
        if now.saturating_duration_since(asked.at) > GIVE_UP_AFTER {
            let still = if behind.is_empty() {
                "no agent is reporting".to_string()
            } else {
                format!("still on {}", behind.join(", "))
            };
            return Some(view(
                "failed",
                format!(
                    "not finished after {} minutes: {still}",
                    GIVE_UP_AFTER.as_secs() / 60
                ),
            ));
        }
        Some(view("pending", String::new()))
    }
}

impl IUpgradeService for UpgradeServiceImp {
    fn attach(&self, source_id: &str, agent: &str, features: &[String], to: ToAgent) -> u64 {
        let mut state = self.lock();
        state.next += 1;
        let id = state.next;
        let upgrade = features.iter().any(|f| f == "upgrade");
        state
            .sessions
            .entry(source_id.to_string())
            .or_default()
            .insert(agent.to_string(), Session { id, upgrade, to });
        id
    }

    fn detach(&self, source_id: &str, agent: &str, session: u64) {
        let mut state = self.lock();
        if let Some(agents) = state.sessions.get_mut(source_id)
            && agents.get(agent).is_some_and(|s| s.id == session)
        {
            agents.remove(agent);
        }
    }

    fn can_upgrade(&self, source_id: &str) -> bool {
        self.lock()
            .sessions
            .get(source_id)
            .is_some_and(|agents| agents.values().any(|s| s.upgrade))
    }

    fn request(&self, source_id: &str, version: &str) -> Result<usize, UpgradeError> {
        if !hermes_proto::valid_version(version) {
            return Err(UpgradeError::BadVersion);
        }
        let mut state = self.lock();
        let targets: Vec<ToAgent> = state
            .sessions
            .get(source_id)
            .into_iter()
            .flatten()
            .filter(|(_, s)| s.upgrade)
            .map(|(_, s)| s.to.clone())
            .collect();
        if targets.is_empty() {
            return Err(UpgradeError::NoCapableAgent);
        }
        let message = HubMessage {
            message: Some(hub_message::Message::Upgrade(Upgrade {
                version: version.to_string(),
            })),
        };
        let asked = targets
            .into_iter()
            .filter(|to| to.try_send(Ok(message.clone())).is_ok())
            .count();
        if asked == 0 {
            return Err(UpgradeError::NoCapableAgent);
        }
        state.asked.insert(
            source_id.to_string(),
            Asked {
                version: version.to_string(),
                at: Instant::now(),
                failure: None,
                finished: None,
            },
        );
        Ok(asked)
    }

    fn report(&self, source_id: &str, agent: &str, status: &UpgradeStatus) {
        info!(
            "upgrade of source {source_id} to {}: agent {agent} says {:?} {}",
            status.version,
            status.state(),
            status.message
        );
        if status.state() != UpgradeState::Failed {
            return;
        }
        if let Some(asked) = self
            .lock()
            .asked
            .get_mut(source_id)
            .filter(|a| a.version == status.version)
        {
            asked.failure = Some(if status.message.is_empty() {
                "the agent refused".into()
            } else {
                status.message.clone()
            });
        }
    }

    fn view(&self, source_id: &str, agents: &[AgentInfo]) -> Option<UpgradeView> {
        self.view_at(Instant::now(), source_id, agents)
    }

    fn forget(&self, source_id: &str) {
        let mut state = self.lock();
        state.asked.remove(source_id);
        state.sessions.remove(source_id);
    }
}

pub type SharedUpgrades = Arc<dyn IUpgradeService>;

#[cfg(test)]
#[path = "../../tests/unit/upgrades.rs"]
mod tests;
