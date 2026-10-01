//! `IEngine` is the interface the controllers depend on (`ack`, `set_settings`, `evaluate`); `EngineImp` is the stateful
//! lifecycle behind it: it asks `judge()` what should be true and persists/publishes the difference — opening, updating
//! and resolving alerts, recording heartbeats. The actual evaluation is `judge()`'s job, not this file's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Deserialize;
use serde_json::json;
use tracing::warn;

use super::judge::{Beat, Want, judge};
use super::rule::{Rule, defaults};
use crate::database::{IAlertDAO, IBeatDAO, ISettingsDAO, ISourceDAO, now_ms};
use crate::model::Alert;
use crate::services::IStore;

/// How long a condition must stay away before its alert resolves. Without it a crash-looping service, which alternates between running
/// and failing, would open and close an alert every few seconds.
pub(crate) const HOLD_DOWN_MS: i64 = 15_000;

pub trait IEngine: Send + Sync {
    /// Reads the alert rules out of the settings JSON (`{"rules":[{"id","enabled","value","crit"}]}`).
    fn set_settings(&self, raw: &[u8]);
    /// Evaluates the live state once. `app.rs` calls this once a second for as long as the process runs.
    fn evaluate(&self, now: i64);
    /// Marks an alert acknowledged (by whom, and when is now) and tells all browsers.
    fn ack(&self, id: i64, by: &str) -> bool;
}

#[derive(Default)]
struct State {
    rules: HashMap<String, Rule>,
    /// by alert key
    active: HashMap<String, Alert>,
    /// alert key -> when its condition first went away (for the hold-down)
    clear_at: HashMap<String, i64>,
    /// node id -> last recorded heartbeat status
    last_beat: HashMap<String, Beat>,
}

pub struct EngineImp {
    store: Arc<dyn IStore>,
    alerts: Arc<dyn IAlertDAO>,
    sources: Arc<dyn ISourceDAO>,
    beats: Arc<dyn IBeatDAO>,
    settings: Arc<dyn ISettingsDAO>,
    state: Mutex<State>,
}

impl EngineImp {
    pub fn new(
        store: Arc<dyn IStore>,
        alerts: Arc<dyn IAlertDAO>,
        sources: Arc<dyn ISourceDAO>,
        beats: Arc<dyn IBeatDAO>,
        settings: Arc<dyn ISettingsDAO>,
    ) -> Self {
        let mut state = State {
            rules: defaults(),
            ..State::default()
        };
        for a in alerts.active_alerts().unwrap_or_default() {
            state.active.insert(a.key.clone(), a);
        }
        let engine = Self {
            store,
            alerts,
            sources,
            beats,
            settings,
            state: Mutex::new(state),
        };
        if let Some(raw) = engine.settings.get_setting("settings") {
            engine.set_settings(raw.as_bytes());
        }
        engine
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, alert: &Alert, is_new: bool) {
        self.store
            .publish(&json!({"type": "alert", "alert": alert, "isNew": is_new}));
    }

    fn reconcile(&self, st: &mut State, mut desired: HashMap<String, Want>, now: i64) {
        let keys: Vec<String> = st.active.keys().cloned().collect();
        for key in keys {
            let Some(want) = desired.remove(&key) else {
                let since = *st.clear_at.entry(key.clone()).or_insert(now);
                if since == now || now - since < HOLD_DOWN_MS {
                    continue; // just went away, or still waiting out the hold-down
                }
                st.clear_at.remove(&key);
                let Some(mut alert) = st.active.remove(&key) else {
                    continue;
                };
                alert.resolved_ts = Some(since); // it was healthy from the moment the condition went away
                if let Err(e) = self.alerts.update_alert(&alert) {
                    warn!("rules: resolve alert: {e:#}");
                }
                self.publish(&alert, false);
                continue;
            };
            st.clear_at.remove(&key); // the condition is back: restart the hold-down
            if let Some(alert) = st.active.get_mut(&key)
                && (alert.sev != want.sev
                    || alert.title != want.title
                    || alert.detail != want.detail)
            {
                alert.sev = want.sev;
                alert.title = want.title;
                alert.detail = want.detail;
                let _ = self.alerts.update_alert(alert);
                self.publish(alert, false);
            }
        }
        for (key, want) in desired {
            let mut alert = Alert {
                key: key.clone(),
                sev: want.sev,
                node_id: want.node_id,
                title: want.title,
                detail: want.detail,
                ts: now,
                snapshot: want.snapshot,
                ..Alert::default()
            };
            if let Err(e) = self.alerts.insert_alert(&mut alert) {
                warn!("rules: insert alert: {e:#}");
                continue;
            }
            self.publish(&alert, true);
            st.active.insert(key, alert);
        }
    }
}

impl IEngine for EngineImp {
    fn set_settings(&self, raw: &[u8]) {
        #[derive(Deserialize)]
        struct Settings {
            #[serde(default)]
            rules: Vec<RuleSetting>,
        }
        #[derive(Deserialize)]
        struct RuleSetting {
            id: String,
            #[serde(default)]
            enabled: bool,
            value: Option<f64>,
            crit: Option<f64>,
        }
        let Ok(settings) = serde_json::from_slice::<Settings>(raw) else {
            return;
        };
        let mut rules = defaults();
        for r in settings.rules {
            let cur = rules.entry(r.id).or_default();
            cur.enabled = r.enabled;
            cur.value = r.value.unwrap_or(cur.value);
            cur.crit = r.crit.unwrap_or(cur.crit);
        }
        self.lock().rules = rules;
    }

    fn evaluate(&self, now: i64) {
        let nodes = self.store.nodes();
        let sources = self.sources.list_sources().unwrap_or_default();
        let mut st = self.lock();
        let rules = st.rules.clone();
        let (desired, beats) = judge(&nodes, &sources, &rules);
        for (id, beat) in beats {
            if st.last_beat.get(&id) != Some(&beat) {
                let _ = self.beats.insert_beat(&id, now, beat.as_str());
                st.last_beat.insert(id, beat);
            }
        }
        self.reconcile(&mut st, desired, now);
    }

    fn ack(&self, id: i64, by: &str) -> bool {
        let Ok(Some(mut alert)) = self.alerts.get_alert(id) else {
            return false;
        };
        alert.ack = true;
        alert.ack_by = by.to_string();
        alert.ack_ts = Some(now_ms());
        if self.alerts.update_alert(&alert).is_err() {
            return false;
        }
        if let Some(cur) = self
            .lock()
            .active
            .get_mut(&alert.key)
            .filter(|cur| cur.id == alert.id)
        {
            cur.ack = true;
            cur.ack_by.clone_from(&alert.ack_by);
            cur.ack_ts = alert.ack_ts;
        }
        self.publish(&alert, false);
        true
    }
}
