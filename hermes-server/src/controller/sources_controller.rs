//! Adding, listing and removing the clusters/agents the hub watches.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;

use super::{Admin, Shared, fail, internal, json_response, ok, parse};
use crate::manifest::{self, AgentParams};
use crate::model::{
    AddSourceRequest, AddSourceResponse, AgentView, Source, SourceView, UpgradeRequest,
};
use crate::services::UpgradeError;
use crate::version;

pub(super) async fn list_sources(State(st): State<Shared>, _: Admin) -> Response {
    let list = match st.db.sources.list_sources() {
        Ok(list) => list,
        Err(e) => return internal(e),
    };
    let expected = st.latest_agent().await;
    let views: Vec<SourceView> = list
        .into_iter()
        .map(|source| {
            let infos = st.ingest.agents(&source.id);
            let upgrade = st.upgrades.view(&source.id, &infos);
            let agents = infos
                .into_iter()
                .map(|a| AgentView {
                    outdated: expected
                        .as_ref()
                        .is_some_and(|expected| version::is_outdated(&a.version, expected)),
                    protocol_outdated: a.protocol < hermes_proto::PROTOCOL,
                    id: a.id,
                    version: a.version,
                    collector: a.collector,
                    host: a.host,
                    seen_ago: a.seen_ago.as_secs(),
                })
                .collect();
            let can_upgrade = st.upgrades.can_upgrade(&source.id);
            SourceView {
                source,
                agents,
                expected_agent: expected.as_ref().map(ToString::to_string),
                can_upgrade,
                upgrade,
            }
        })
        .collect();
    json_response(StatusCode::OK, views)
}

fn rand_hex(bytes: usize) -> String {
    hex::encode((0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>())
}

/// The rules that don't need the database: a known name, a supported type, `flows` only where it applies, a `hubUrl` given.
#[allow(clippy::result_large_err)]
fn validate_add_source(req: &AddSourceRequest) -> Result<(), Response> {
    if req.name.trim().is_empty() || req.kind.is_empty() {
        return Err(fail(StatusCode::BAD_REQUEST, "name and type are required"));
    }
    if !crate::model::is_agent_type(&req.kind)
        || !super::live_controller::source_types().contains(&req.kind.as_str())
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            &format!(
                "source type {:?} is not supported yet (supported: [{}])",
                req.kind,
                super::live_controller::source_types().join(" ")
            ),
        ));
    }
    if req.flows && !manifest::supports_flows(&req.kind) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "who talks to whom is only available for Kubernetes sources so far",
        ));
    }
    if req.hub_url.trim().is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "hubUrl is required: the address of this hub as seen from the cluster",
        ));
    }
    Ok(())
}

/// A fresh source from a request already checked by `validate_add_source`: a new id, a new agent token, waiting to be reached.
fn new_source(req: &AddSourceRequest) -> Source {
    Source {
        id: format!("s{}", rand_hex(6)),
        name: req.name.trim().to_string(),
        kind: req.kind.clone(),
        endpoint: req.endpoint.clone(),
        state: "pending".into(),
        auth: "Agent token".into(),
        info: "waiting for the agent to report".into(),
        secret: rand_hex(24), // the agent token
        builtin: false,
    }
}

/// The images an install manifest points at, and how the cluster logs in to get them.
struct AgentImages {
    linux: String,
    windows: String,
    pull_secret: String,
    /// Kubernetes: the pull secret's content, so the manifest creates it. Empty for a public registry or when the secret is made by hand.
    pull_secret_data: String,
    /// Swarm: the registry the manager has to `docker login` to.
    login: Option<(String, String)>,
}

impl AgentImages {
    fn login_hint(&self, kind: &str, hint: &str) -> String {
        if !self.pull_secret_data.is_empty() {
            return "Save it as agent.yaml and run: kubectl apply -f agent.yaml. The manifest also creates the secret the cluster pulls the image with: it holds the registry login, so keep the file private (and use an account that can only pull).".into();
        }
        match (&self.login, kind == crate::model::TYPE_SWARM_AGENT) {
            (Some((host, user)), true) => format!(
                "{hint} This registry needs a login: on the manager run docker login {host} -u {user} first, and use --with-registry-auth."
            ),
            _ => hint.to_string(),
        }
    }
}

/// With a registry set up, the images come from it, at the version chosen in the request; without one, from the hub's environment.
#[allow(clippy::result_large_err)]
fn agent_images(
    st: &Shared,
    registry: &crate::model::RegistryConfig,
    req: &AddSourceRequest,
) -> Result<AgentImages, Response> {
    let settings = &st.settings;
    // the project's own registry was never chosen by anyone: without a version it is as if there were none
    if !registry.is_set() || (registry.implicit && req.version.trim().is_empty()) {
        return Ok(AgentImages {
            linux: settings.agent_image.clone(),
            windows: settings.agent_image_windows.clone(),
            pull_secret: settings.agent_pull_secret.clone(),
            pull_secret_data: String::new(),
            login: None,
        });
    }
    let version = req.version.trim();
    if version.is_empty() || version.contains(['/', ':', ' ']) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "choose the agent version to install",
        ));
    }
    let windows = if req.windows {
        format!(
            "{}:{version}-ltsc2022",
            registry.image(registry.windows_repo())
        )
    } else {
        String::new()
    };
    let basic = registry.auth == "basic";
    let kubernetes = req.kind == crate::model::TYPE_KUBERNETES_AGENT;
    Ok(AgentImages {
        linux: format!("{}:{version}", registry.image(registry.linux_repo())),
        windows,
        pull_secret: if basic && kubernetes {
            "hermes-registry".into()
        } else {
            String::new()
        },
        pull_secret_data: if basic && kubernetes {
            manifest::docker_config(registry.image_host(), &registry.username, &registry.secret)
        } else {
            String::new()
        },
        login: basic.then(|| (registry.image_host().to_string(), registry.username.clone())),
    })
}

pub(super) async fn add_source(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let req: AddSourceRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "name and type are required"),
    };
    if let Err(r) = validate_add_source(&req) {
        return r;
    }

    let source = new_source(&req);
    let hub_url = req.hub_url.trim().trim_end_matches('/');
    let registry = st.db.registry.get_registry();
    let images = match agent_images(&st, &registry, &req) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let params = AgentParams {
        image: &images.linux,
        windows_image: &images.windows,
        pull_secret: &images.pull_secret,
        pull_secret_data: &images.pull_secret_data,
        flows: req.flows,
        upgrades: req.upgrades,
        hub_url,
        source_id: &source.id,
        source_name: &source.name,
        token: &source.secret,
    };
    let (install, hint) = match manifest::render(&req.kind, &params) {
        Ok(r) => r,
        Err(e) => return internal(e),
    };
    let hint = images.login_hint(&req.kind, hint);
    if let Err(e) = st.db.sources.insert_source(&source) {
        return internal(e);
    }
    json_response(
        StatusCode::CREATED,
        AddSourceResponse {
            source,
            install,
            hint,
        },
    )
}

pub(super) async fn remove_source(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<String>,
) -> Response {
    if id == "demo" {
        return fail(
            StatusCode::FORBIDDEN,
            "the development fixture cannot be removed",
        );
    }
    st.guard.remove_source(&id);
    st.upgrades.forget(&id);
    if let Err(e) = st.db.sources.delete_source(&id) {
        return internal(e);
    }
    st.store.publish(&json!({"type": "sources"}));
    ok()
}

/// Changes the agent version of a source: the agents are told, and each changes its own image. Nothing here touches the cluster.
pub(super) async fn upgrade_source(
    State(st): State<Shared>,
    Admin(user): Admin,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let req: UpgradeRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let version = req.version.trim();
    match st.db.sources.list_sources() {
        Ok(list) if list.iter().any(|s| s.id == id && s.is_agent()) => {}
        Ok(_) => return fail(StatusCode::NOT_FOUND, "no such source"),
        Err(e) => return internal(e),
    }
    if !hermes_proto::valid_version(version) {
        return fail(StatusCode::BAD_REQUEST, "choose a version like 1.2.3");
    }
    // with a registry set up, only what it really holds can be chosen: a typo would leave the cluster unable to pull the image
    let registry = st.db.registry.get_registry();
    if registry.is_set() && !registry.implicit {
        match st.registry.tags(&registry, registry.linux_repo()).await {
            Ok(tags) if tags.iter().any(|t| t == version) => {}
            Ok(_) => {
                return fail(
                    StatusCode::BAD_REQUEST,
                    &format!("{version} is not in the registry"),
                );
            }
            Err(e) => {
                return fail(
                    StatusCode::BAD_GATEWAY,
                    &format!("cannot check the registry: {e}"),
                );
            }
        }
    }
    match st.upgrades.request(&id, version) {
        Ok(agents) => {
            tracing::info!(
                "{} asked source {id} to run agent version {version} ({agents} agent(s) told)",
                user.username
            );
            st.store.publish(&json!({"type": "sources"}));
            json_response(StatusCode::ACCEPTED, json!({"ok": true, "agents": agents}))
        }
        Err(UpgradeError::BadVersion) => {
            fail(StatusCode::BAD_REQUEST, "choose a version like 1.2.3")
        }
        Err(UpgradeError::NoCapableAgent) => fail(
            StatusCode::CONFLICT,
            "no connected agent of this source was installed to upgrade itself: add the source again with \"Allow upgrades from the dashboard\" ticked and apply that manifest once",
        ),
    }
}
