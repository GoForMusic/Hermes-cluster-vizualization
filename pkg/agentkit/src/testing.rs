//! A sink that only remembers what it was told, for the tests of collectors.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use hermes_proto::v1::{CollectorState, Edge, Node, Own};
use prost_types::Struct;

use crate::ISink;

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Topology(Vec<Node>, Vec<Edge>),
    Status(String, Own, String),
    Meta(String, Struct),
    Metrics(HashMap<String, HashMap<String, f64>>, HashMap<String, f64>),
    Report(CollectorState, String),
    Alive(Vec<String>),
    Contribution(Vec<Node>),
}

#[derive(Default)]
pub struct RecordingSink {
    calls: Mutex<Vec<Call>>,
}

impl RecordingSink {
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn push(&self, call: Call) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
    }
}

impl ISink for RecordingSink {
    fn set_topology(&self, nodes: Vec<Node>, edges: Vec<Edge>) {
        self.push(Call::Topology(nodes, edges));
    }
    fn status(&self, id: &str, own: Own, reason: &str) {
        self.push(Call::Status(id.into(), own, reason.into()));
    }
    fn meta(&self, id: &str, patch: Struct) {
        self.push(Call::Meta(id.into(), patch));
    }
    fn metrics(&self, nodes: HashMap<String, HashMap<String, f64>>, edges: HashMap<String, f64>) {
        self.push(Call::Metrics(nodes, edges));
    }
    fn report(&self, state: CollectorState, info: &str) {
        self.push(Call::Report(state, info.into()));
    }
    fn alive(&self, ids: Vec<String>) {
        self.push(Call::Alive(ids));
    }
    fn contribute(&self, nodes: Vec<Node>) {
        self.push(Call::Contribution(nodes));
    }
}
