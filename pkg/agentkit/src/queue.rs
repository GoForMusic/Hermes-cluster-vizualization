//! `Uplink`'s outbox: a pure in-memory state machine, no I/O. It queues events, keeps the latest snapshot/alive/contribution/report so
//! a resync can repeat what is true now (not what was true when it happened), and hands over a batch when asked.

use std::collections::{HashMap, VecDeque};

use hermes_proto::v1::{
    Alive, CollectorState, Contribution, Edge, Event, Flow, Flows, Metrics, Node, Own, Report,
    Snapshot, Status, Values, event,
};
use prost_types::Struct;

/// Events waiting for the next flush. The oldest go first when the hub is unreachable for long.
const MAX_QUEUE: usize = 500;

#[derive(Default)]
pub(crate) struct State {
    queue: VecDeque<Event>,
    /// The last snapshot, kept current by status and meta changes, so that after a hub restart we send what is true now.
    last: Option<Snapshot>,
    pub(crate) resync: bool,
    report: Option<Report>,
    report_dirty: bool,
    alive: Option<Alive>,
    contribution: Option<Contribution>,
}

fn event(kind: event::Kind) -> Event {
    Event { kind: Some(kind) }
}

impl State {
    fn push(&mut self, ev: Event) {
        if self.queue.len() >= MAX_QUEUE {
            self.queue.pop_front();
        }
        self.queue.push_back(ev);
    }

    /// Replace the newest queued event when it is of the same kind: only the newest sample matters.
    fn push_latest(&mut self, ev: Event, same_kind: fn(&event::Kind) -> bool) {
        match self.queue.back_mut() {
            Some(Event { kind: Some(k) }) if same_kind(k) => {
                *self.queue.back_mut().expect("just matched") = ev
            }
            _ => self.push(ev),
        }
    }

    pub(crate) fn set_topology(&mut self, nodes: Vec<Node>, edges: Vec<Edge>) {
        let snapshot = Snapshot { nodes, edges };
        self.last = Some(snapshot.clone());
        self.queue.clear(); // a snapshot supersedes everything queued before it
        self.queue.push_back(event(event::Kind::Snapshot(snapshot)));
    }

    pub(crate) fn status(&mut self, id: &str, own: Own, reason: &str) {
        self.patch_last(id, |n| {
            n.own = own.into();
            n.reason = reason.to_string();
        });
        self.push(event(event::Kind::Status(Status {
            id: id.to_string(),
            own: own.into(),
            reason: reason.to_string(),
        })));
    }

    pub(crate) fn meta(&mut self, id: &str, patch: Struct) {
        self.patch_last(id, |n| {
            n.meta
                .get_or_insert_default()
                .fields
                .extend(patch.fields.clone())
        });
        self.push(event(event::Kind::Meta(hermes_proto::v1::Meta {
            id: id.to_string(),
            patch: Some(patch.clone()),
        })));
    }

    fn patch_last(&mut self, id: &str, change: impl FnOnce(&mut Node)) {
        if let Some(node) = self
            .last
            .as_mut()
            .and_then(|s| s.nodes.iter_mut().find(|n| n.id == id))
        {
            change(node);
        }
    }

    pub(crate) fn metrics(
        &mut self,
        nodes: HashMap<String, HashMap<String, f64>>,
        edges: HashMap<String, f64>,
    ) {
        let nodes = nodes
            .into_iter()
            .map(|(id, values)| (id, Values { values }))
            .collect();
        self.push_latest(event(event::Kind::Metrics(Metrics { nodes, edges })), |k| {
            matches!(k, event::Kind::Metrics(_))
        });
    }

    pub(crate) fn alive(&mut self, ids: Vec<String>) {
        let alive = Alive { ids };
        self.alive = Some(alive.clone());
        self.push_latest(event(event::Kind::Alive(alive)), |k| {
            matches!(k, event::Kind::Alive(_))
        });
    }

    pub(crate) fn flows(&mut self, flows: Vec<Flow>) {
        self.push_latest(event(event::Kind::Flows(Flows { flows })), |k| {
            matches!(k, event::Kind::Flows(_))
        });
    }

    pub(crate) fn contribute(&mut self, nodes: Vec<Node>) {
        let contribution = Contribution { nodes };
        self.contribution = Some(contribution.clone());
        self.push_latest(event(event::Kind::Contribution(contribution)), |k| {
            matches!(k, event::Kind::Contribution(_))
        });
    }

    /// Only a change of state is told to the hub (and again on resync), not every repeat of the same one.
    pub(crate) fn report(&mut self, state: CollectorState, info: &str) {
        if self.report.as_ref().is_some_and(|r| r.state() == state) {
            return;
        }
        self.report = Some(Report {
            state: state.into(),
            info: info.to_string(),
        });
        self.report_dirty = true;
    }

    /// Everything to send now. After a resync that also means what the hub has to learn again: the snapshot, the workloads
    /// that run on this host, and the collector's state.
    pub(crate) fn take_batch(&mut self) -> Vec<Event> {
        let mut events: Vec<Event> = self.queue.drain(..).collect();
        let resync = std::mem::take(&mut self.resync);
        if resync {
            if let Some(snapshot) = &self.last
                && !matches!(
                    events.first(),
                    Some(Event {
                        kind: Some(event::Kind::Snapshot(_))
                    })
                )
            {
                events.insert(0, event(event::Kind::Snapshot(snapshot.clone())));
            }
            if let Some(alive) = &self.alive
                && !events
                    .iter()
                    .any(|e| matches!(e.kind, Some(event::Kind::Alive(_))))
            {
                events.push(event(event::Kind::Alive(alive.clone())));
            }
            if let Some(contribution) = &self.contribution
                && !events
                    .iter()
                    .any(|e| matches!(e.kind, Some(event::Kind::Contribution(_))))
            {
                events.push(event(event::Kind::Contribution(contribution.clone())));
            }
        }
        if (resync || self.report_dirty)
            && let Some(report) = &self.report
        {
            events.push(event(event::Kind::Report(report.clone())));
        }
        self.report_dirty = false;
        events
    }
}

#[cfg(test)]
#[path = "../tests/unit/queue.rs"]
mod tests;
