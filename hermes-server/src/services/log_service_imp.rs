//! Reading the log of a container through the agents. The hub keeps nothing: a request goes to an agent over the stream it keeps open, the
//! lines that come back are handed to whoever is watching, and a viewer that leaves cancels the request. Only an agent that was installed
//! to (its hello says `logs`) is asked, one at a time: an agent that is not the one for this workload says so (`not_here`) and the next is asked.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use hermes_proto::v1::{HubMessage, LogCancel, LogChunk, LogRequest, hub_message};
use tokio::sync::mpsc;
use tracing::debug;

use crate::services::ToAgent;

/// An agent has this long to start answering.
const FIRST_ANSWER_WITHIN: Duration = Duration::from_secs(20);
/// A viewer that does not read for this many chunks is cut off, rather than the agent's session being held up.
const PER_REQUEST_BUFFER: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogOptions {
    pub namespace: String,
    pub name: String,
    pub container: String,
    pub tail: u32,
    pub follow: bool,
    pub previous: bool,
    pub since_seconds: u32,
    pub timestamps: bool,
    pub task: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LogEvent {
    Lines(Vec<String>),
    /// The log ended; nothing more comes.
    End,
    /// It failed, and why. Nothing more comes.
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum LogOpenError {
    /// No connected agent of the source was installed to read logs.
    NoCapableAgent,
    /// The agents that can read logs were asked, and none of them has this workload (the reasons they gave).
    NotFound(String),
    /// An agent was asked and did not answer, or the link broke.
    Failed(String),
}

struct Session {
    id: u64,
    logs: bool,
    to: ToAgent,
}

struct Request {
    source: String,
    agent: String,
    to: mpsc::Sender<Item>,
}

/// What the service hands to the viewer's side of a request, apart from the log itself.
enum Item {
    Event(LogEvent),
    /// The agent said this is not its workload.
    NotHere(String),
}

#[derive(Default)]
struct State {
    next_session: u64,
    next_request: u64,
    sessions: HashMap<String, HashMap<String, Session>>,
    requests: HashMap<u64, Request>,
}

#[derive(Default)]
pub struct LogServiceImp {
    state: Mutex<State>,
}

impl LogServiceImp {
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An agent's session opened. Returns a token for `detach`.
    pub fn attach(&self, source: &str, agent: &str, features: &[String], to: ToAgent) -> u64 {
        let mut st = self.lock();
        st.next_session += 1;
        let id = st.next_session;
        let logs = features.iter().any(|f| f == "logs");
        st.sessions
            .entry(source.to_string())
            .or_default()
            .insert(agent.to_string(), Session { id, logs, to });
        id
    }

    /// The session ended: what was being read through it is over. A newer session of the same agent is left alone.
    pub fn detach(&self, source: &str, agent: &str, session: u64) {
        let mut st = self.lock();
        let Some(agents) = st.sessions.get_mut(source) else {
            return;
        };
        if agents.get(agent).is_some_and(|s| s.id == session) {
            agents.remove(agent);
        }
        let gone: Vec<u64> = st
            .requests
            .iter()
            .filter(|(_, r)| r.source == source && r.agent == agent)
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(r) = st.requests.remove(&id) {
                let _ = r.to.try_send(Item::Event(LogEvent::Failed(
                    "the agent disconnected".into(),
                )));
            }
        }
    }

    /// Some connected agent of the source was installed to read logs.
    pub fn can_read(&self, source: &str) -> bool {
        self.lock()
            .sessions
            .get(source)
            .is_some_and(|a| a.values().any(|s| s.logs))
    }

    /// The source is gone.
    pub fn forget(&self, source: &str) {
        let mut st = self.lock();
        st.sessions.remove(source);
        st.requests.retain(|_, r| r.source != source);
    }

    /// What an agent sent for a request. Only the agent the request went to may answer it.
    pub fn chunk(&self, source: &str, agent: &str, chunk: LogChunk) {
        let mut st = self.lock();
        let Some(request) = st.requests.get(&chunk.id) else {
            return;
        };
        if request.source != source || request.agent != agent {
            return;
        }
        let item = if chunk.not_here {
            Item::NotHere(chunk.error)
        } else if !chunk.error.is_empty() {
            Item::Event(LogEvent::Failed(chunk.error))
        } else if chunk.end {
            Item::Event(LogEvent::End)
        } else {
            Item::Event(LogEvent::Lines(chunk.lines))
        };
        let last = !matches!(item, Item::Event(LogEvent::Lines(_)));
        match request.to.try_send(item) {
            Ok(()) if !last => {}
            Ok(()) => {
                st.requests.remove(&chunk.id);
            }
            // the viewer does not keep up (or is gone): stop it here and tell the agent
            Err(_) => {
                if let Some(r) = st.requests.remove(&chunk.id)
                    && let Some(session) = st.sessions.get(&r.source).and_then(|a| a.get(&r.agent))
                {
                    let _ = session.to.try_send(Ok(cancel(chunk.id)));
                }
            }
        }
    }

    /// Asks the agents of the source, one by one, for a log. The stream that is returned already has its first answer.
    pub async fn open(
        self: &Arc<Self>,
        source: &str,
        options: &LogOptions,
    ) -> Result<LogStream, LogOpenError> {
        let candidates: Vec<(String, ToAgent)> = {
            let st = self.lock();
            let mut c: Vec<_> = st
                .sessions
                .get(source)
                .map(|a| {
                    a.iter()
                        .filter(|(_, s)| s.logs)
                        .map(|(n, s)| (n.clone(), s.to.clone()))
                        .collect()
                })
                .unwrap_or_default();
            c.sort_by(|a: &(String, ToAgent), b| a.0.cmp(&b.0));
            c
        };
        if candidates.is_empty() {
            return Err(LogOpenError::NoCapableAgent);
        }
        let mut reasons = Vec::new();
        let mut failure = None;
        for (agent, to) in candidates {
            let (tx, mut rx) = mpsc::channel(PER_REQUEST_BUFFER);
            let id = {
                let mut st = self.lock();
                st.next_request += 1;
                let id = st.next_request;
                st.requests.insert(
                    id,
                    Request {
                        source: source.to_string(),
                        agent: agent.clone(),
                        to: tx,
                    },
                );
                id
            };
            let ask = HubMessage {
                message: Some(hub_message::Message::LogRequest(LogRequest {
                    id,
                    namespace: options.namespace.clone(),
                    name: options.name.clone(),
                    container: options.container.clone(),
                    tail: options.tail,
                    follow: options.follow,
                    previous: options.previous,
                    since_seconds: options.since_seconds,
                    timestamps: options.timestamps,
                    task: options.task.clone(),
                })),
            };
            if to.send(Ok(ask)).await.is_err() {
                self.lock().requests.remove(&id);
                failure = Some("the agent's link broke".to_string());
                continue;
            }
            match tokio::time::timeout(FIRST_ANSWER_WITHIN, rx.recv()).await {
                Ok(Some(Item::NotHere(why))) => {
                    self.lock().requests.remove(&id);
                    debug!(
                        "agent {agent} is not the one for {}/{}: {why}",
                        options.namespace, options.name
                    );
                    reasons.push(why);
                }
                Ok(Some(Item::Event(first))) => {
                    return Ok(LogStream {
                        service: self.clone(),
                        id,
                        source: source.to_string(),
                        agent,
                        rx,
                        first: Some(first),
                        done: false,
                    });
                }
                Ok(None) => {
                    failure = Some("the agent disconnected".to_string());
                }
                Err(_) => {
                    self.cancel(source, &agent, id);
                    failure = Some(format!(
                        "the agent did not answer in {} s",
                        FIRST_ANSWER_WITHIN.as_secs()
                    ));
                }
            }
        }
        match failure {
            Some(why) => Err(LogOpenError::Failed(why)),
            None => {
                reasons.dedup();
                Err(LogOpenError::NotFound(reasons.join("; ")))
            }
        }
    }

    fn cancel(&self, source: &str, agent: &str, id: u64) {
        let mut st = self.lock();
        st.requests.remove(&id);
        if let Some(session) = st.sessions.get(source).and_then(|a| a.get(agent)) {
            let _ = session.to.try_send(Ok(cancel(id)));
        }
    }
}

fn cancel(id: u64) -> HubMessage {
    HubMessage {
        message: Some(hub_message::Message::LogCancel(LogCancel { id })),
    }
}

/// A log being read. Dropping it before it ended tells the agent to stop.
pub struct LogStream {
    service: Arc<LogServiceImp>,
    id: u64,
    source: String,
    agent: String,
    rx: mpsc::Receiver<Item>,
    first: Option<LogEvent>,
    done: bool,
}

impl LogStream {
    /// The next lines, or how it ended. `None` after that.
    pub async fn next(&mut self) -> Option<LogEvent> {
        if self.done {
            return None;
        }
        let event = match self.first.take() {
            Some(e) => e,
            None => match self.rx.recv().await {
                Some(Item::Event(e)) => e,
                Some(Item::NotHere(why)) => LogEvent::Failed(why),
                None => LogEvent::Failed(
                    "the log was cut: it was not read fast enough, or the agent left".into(),
                ),
            },
        };
        if !matches!(event, LogEvent::Lines(_)) {
            self.done = true;
        }
        Some(event)
    }
}

impl Drop for LogStream {
    fn drop(&mut self) {
        if !self.done {
            self.service.cancel(&self.source, &self.agent, self.id);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/log_service.rs"]
mod tests;
