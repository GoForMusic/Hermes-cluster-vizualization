use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One thing the hub watches: a cluster it reads itself (pull) or an agent that reports in (push).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Source {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub endpoint: String,
    pub auth: String,
    pub state: String, // connected | pending | error | duplicate
    pub info: String,
    #[serde(skip_serializing_if = "is_false")]
    #[ts(as = "Option<bool>", optional)]
    pub builtin: bool,
    /// The kubeconfig or agent token: stored, but never sent to the browser.
    #[serde(skip)]
    #[ts(skip)]
    pub secret: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

pub const TYPE_KUBERNETES_AGENT: &str = "Kubernetes (agent)";
pub const TYPE_SWARM_AGENT: &str = "Docker Swarm (agent)";

impl Source {
    /// "(agent)" sources are push: a small agent inside the environment reports to the hub.
    pub fn is_agent(&self) -> bool {
        is_agent_type(&self.kind)
    }

    /// The orchestrator the source talks to, as the nodes it reports name it.
    pub fn provider(&self) -> &'static str {
        let t = self.kind.to_lowercase();
        if t.contains("swarm") {
            "swarm"
        } else if t.contains("nomad") {
            "nomad"
        } else {
            "kubernetes"
        }
    }
}

pub fn is_agent_type(kind: &str) -> bool {
    kind.ends_with("(agent)")
}
