//! The accounts and groups the admin page manages: `/api/users` and `/api/groups`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Deserialize;

use super::auth_controller::blocking;
use super::{Admin, Shared, fail, internal, json_response, ok, parse};
use crate::services::{AccessError, UserChanges};

fn error(e: AccessError) -> Response {
    let code = match e {
        AccessError::BadName
        | AccessError::WeakPassword
        | AccessError::BadEmail
        | AccessError::BadGroupName => StatusCode::BAD_REQUEST,
        AccessError::Taken | AccessError::Protected(_) => StatusCode::CONFLICT,
        AccessError::NotFound => StatusCode::NOT_FOUND,
        AccessError::Internal(ref inner) => return internal(inner),
    };
    fail(code, &e.to_string())
}

/// Hashing a password takes a while and a lot of memory: every call runs off the async threads.
async fn run<T: serde::Serialize + Send + 'static>(
    st: &Shared,
    code: StatusCode,
    work: impl FnOnce(&dyn crate::services::IAccess) -> Result<T, AccessError> + Send + 'static,
) -> Response {
    let access = st.access.clone();
    match blocking(move || work(access.as_ref())).await {
        Ok(Ok(v)) => json_response(code, v),
        Ok(Err(e)) => error(e),
        Err(r) => r,
    }
}

pub(super) async fn list_users(State(st): State<Shared>, _: Admin) -> Response {
    run(&st, StatusCode::OK, |a| a.users()).await
}

pub(super) async fn create_user(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct New {
        username: String,
        password: String,
        email: String,
        break_glass: bool,
    }
    let new: New = match parse(&body) {
        Ok(n) => n,
        Err(r) => return r,
    };
    run(&st, StatusCode::CREATED, move |a| {
        a.create_user(&new.username, &new.password, &new.email, new.break_glass)
    })
    .await
}

pub(super) async fn update_user(
    State(st): State<Shared>,
    Admin(acting): Admin,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default, rename_all = "camelCase")]
    struct Change {
        email: Option<String>,
        disabled: Option<bool>,
        break_glass: Option<bool>,
        password: Option<String>,
    }
    let c: Change = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    run(&st, StatusCode::OK, move |a| {
        a.update_user(
            acting.id,
            id,
            UserChanges {
                email: c.email,
                disabled: c.disabled,
                break_glass: c.break_glass,
                password: c.password,
            },
        )
    })
    .await
}

pub(super) async fn delete_user(
    State(st): State<Shared>,
    Admin(acting): Admin,
    Path(id): Path<i64>,
) -> Response {
    match st.access.delete_user(acting.id, id) {
        Ok(()) => ok(),
        Err(e) => error(e),
    }
}

pub(super) async fn list_groups(State(st): State<Shared>, _: Admin) -> Response {
    run(&st, StatusCode::OK, |a| a.groups()).await
}

pub(super) async fn create_group(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct New {
        name: String,
        description: String,
    }
    let new: New = match parse(&body) {
        Ok(n) => n,
        Err(r) => return r,
    };
    run(&st, StatusCode::CREATED, move |a| {
        a.create_group(&new.name, &new.description)
    })
    .await
}

pub(super) async fn update_group(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Change {
        name: Option<String>,
        description: Option<String>,
    }
    let c: Change = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    run(&st, StatusCode::OK, move |a| {
        a.update_group(id, c.name.as_deref(), c.description.as_deref())
    })
    .await
}

pub(super) async fn delete_group(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<i64>,
) -> Response {
    match st.access.delete_group(id) {
        Ok(()) => ok(),
        Err(e) => error(e),
    }
}

pub(super) async fn set_members(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<i64>,
    body: Bytes,
) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Members {
        users: Vec<i64>,
    }
    let m: Members = match parse(&body) {
        Ok(m) => m,
        Err(r) => return r,
    };
    run(&st, StatusCode::OK, move |a| a.set_members(id, &m.users)).await
}
