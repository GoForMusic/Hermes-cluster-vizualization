//! The container registry the agent images come from: its settings, "Test connection", and the versions found in it.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;

use super::{Admin, Shared, fail, internal, json_response, ok, parse};
use crate::model::{RegistryConfig, RegistryInput, RegistryTest, RegistryView};

pub(super) async fn get_registry(State(st): State<Shared>, _: Admin) -> Response {
    json_response(
        StatusCode::OK,
        RegistryView::from(&st.db.registry.get_registry()),
    )
}

pub(super) async fn put_registry(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let input: RegistryInput = match parse(&body) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let config = input.apply(&st.db.registry.get_registry());
    if let Err(e) = st.db.registry.set_registry(&config) {
        return internal(e);
    }
    *st.latest_in_registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None; // another registry: ask again
    json_response(StatusCode::OK, RegistryView::from(&config))
}

/// Tries the settings in the request (not yet saved) against the registry, and lists the agent versions it holds. A request with no
/// body tests what is saved.
pub(super) async fn test_registry(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let stored = st.db.registry.get_registry();
    let config = if body.is_empty() {
        stored
    } else {
        match parse::<RegistryInput>(&body) {
            Ok(input) => input.apply(&stored),
            Err(r) => return r,
        }
    };
    json_response(StatusCode::OK, probe(&st, &config).await)
}

/// The versions of the agent image in the saved registry.
pub(super) async fn registry_versions(State(st): State<Shared>, _: Admin) -> Response {
    let config = st.db.registry.get_registry();
    if !config.is_set() {
        return fail(StatusCode::NOT_FOUND, "no registry is set up");
    }
    json_response(StatusCode::OK, probe(&st, &config).await)
}

async fn probe(st: &Shared, config: &RegistryConfig) -> RegistryTest {
    let client = &st.registry;
    if let Err(e) = client.check(config, config.linux_repo()).await {
        return RegistryTest {
            ok: false,
            message: format!("{e}"),
            versions: vec![],
        };
    }
    match client.tags(config, config.linux_repo()).await {
        Ok(versions) => RegistryTest {
            ok: true,
            message: format!(
                "connected to {} · {} version(s) of {}",
                config.host(),
                versions.len(),
                config.image(config.linux_repo())
            ),
            versions,
        },
        // the registry works, but the agent image is not there (yet): still a working login
        Err(e) => RegistryTest {
            ok: true,
            message: format!("connected to {}, but {e}", config.host()),
            versions: vec![],
        },
    }
}

pub(super) async fn clear_registry(State(st): State<Shared>, _: Admin) -> Response {
    match st.db.registry.set_registry(&RegistryConfig::default()) {
        Ok(()) => ok(),
        Err(e) => internal(e),
    }
}
