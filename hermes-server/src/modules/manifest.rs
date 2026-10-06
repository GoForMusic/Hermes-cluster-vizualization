//! The install manifest of an agent: what the admin applies in the cluster after adding a source. It holds the source's token, so it
//! is shown once, when the source is created.

use anyhow::{Context, Result};
use minijinja::{AutoEscape, Environment, context};

use crate::model::{TYPE_DOCKER_AGENT, TYPE_KUBERNETES_AGENT, TYPE_SWARM_AGENT};

const KUBERNETES: &str = include_str!("../../templates/agent_kubernetes.yaml.j2");
const SWARM: &str = include_str!("../../templates/agent_swarm.yaml.j2");
const DOCKER: &str = include_str!("../../templates/agent_docker.yaml.j2");

const KUBERNETES_HINT: &str = "Save it as agent.yaml and run: kubectl apply -f agent.yaml. The image must be pullable by the cluster; for a private registry create the pull secret in the hermes namespace first.";
const SWARM_HINT: &str = "On a swarm manager save it as agent.yml and run: docker stack deploy --with-registry-auth -c agent.yml hermes. For a private registry run docker login on the manager first; for an image that exists only locally add --resolve-image never. The service is global: every node must be able to get the image.";

const DOCKER_HINT: &str = "Save it as agent.yml on the Docker machine and run: docker compose -f agent.yml up -d. This source is this one machine: for another machine add another source. To say where the machine is: LOCATION=rack-2 docker compose -f agent.yml up -d. For a private registry run docker login on the machine first. When the file has a Windows service too, add --profile linux or --profile windows to say which kind of machine this is.";

/// One agent type's installer: its template, the sentence that explains how to apply it, and whether it can also report who talks to
/// whom. This is the one place a new agent type (Nomad, say) has to be added — `agent_types()`, `render()` and the flows check all
/// read this table instead of each hardcoding its own list of known types.
struct AgentType {
    key: &'static str,
    template: &'static str,
    hint: &'static str,
    /// Kubernetes only, for now: the node agent also reports who talks to whom (it runs in the node's network namespace).
    supports_flows: bool,
}

const AGENT_TYPES: &[AgentType] = &[
    AgentType {
        key: TYPE_KUBERNETES_AGENT,
        template: KUBERNETES,
        hint: KUBERNETES_HINT,
        supports_flows: true,
    },
    AgentType {
        key: TYPE_SWARM_AGENT,
        template: SWARM,
        hint: SWARM_HINT,
        supports_flows: false,
    },
    AgentType {
        key: TYPE_DOCKER_AGENT,
        template: DOCKER,
        hint: DOCKER_HINT,
        supports_flows: false,
    },
];

pub struct AgentParams<'a> {
    pub image: &'a str,
    pub hub_url: &'a str,
    pub source_id: &'a str,
    pub source_name: &'a str,
    pub token: &'a str,
    /// Swarm only: the image for Windows nodes; empty = no service for them.
    pub windows_image: &'a str,
    /// Name of an existing image pull secret in the agent's namespace (private registry); empty = none.
    pub pull_secret: &'a str,
    /// Kubernetes only: the pull secret's content (base64 of a docker config); when given, the manifest creates the secret itself.
    pub pull_secret_data: &'a str,
    /// Kubernetes only: also report who talks to whom. The node agent then runs in the node's network namespace.
    pub flows: bool,
    /// The agents may change their own image when the hub asks: the manifest adds the rights and the switch for it.
    pub upgrades: bool,
}

/// The source types that have an installer, alphabetically.
pub fn agent_types() -> Vec<&'static str> {
    let mut types: Vec<&'static str> = AGENT_TYPES.iter().map(|t| t.key).collect();
    types.sort_unstable();
    types
}

/// Whether this source type's agent can also report who talks to whom.
pub fn supports_flows(source_type: &str) -> bool {
    AGENT_TYPES
        .iter()
        .any(|t| t.key == source_type && t.supports_flows)
}

/// What goes inside a double-quoted YAML string: a name with a quote in it must not break the manifest.
fn quoted(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The content of a Kubernetes image pull secret (`kubernetes.io/dockerconfigjson`), base64 encoded, for one registry login.
pub fn docker_config(host: &str, username: &str, password: &str) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let config = serde_json::json!({"auths": {host: {
        "username": username,
        "password": password,
        "auth": b64.encode(format!("{username}:{password}")),
    }}});
    b64.encode(config.to_string())
}

/// The manifest for a source type and the sentence that says how to apply it.
pub fn render(source_type: &str, p: &AgentParams<'_>) -> Result<(String, &'static str)> {
    let Some(t) = AGENT_TYPES.iter().find(|t| t.key == source_type) else {
        anyhow::bail!("no agent installer for {source_type:?}");
    };
    let (template, hint) = (t.template, t.hint);
    let mut env = Environment::new();
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_keep_trailing_newline(true);
    env.add_template("agent", template)?;
    let out = env
        .get_template("agent")?
        .render(context! {
            Image => p.image,
            HubURL => quoted(p.hub_url),
            SourceID => p.source_id,
            SourceName => quoted(p.source_name),
            Token => p.token,
            WindowsImage => p.windows_image,
            PullSecret => p.pull_secret,
            PullSecretData => p.pull_secret_data,
            Flows => p.flows,
            Upgrades => p.upgrades,
        })
        .context("cannot render the agent manifest")?;
    Ok((out, hint))
}

#[cfg(test)]
#[path = "../../tests/unit/manifest.rs"]
mod tests;
