//! Single sign-on, from the outside: a real hub, a browser-like client, and a stand-in identity provider that checks what a real one checks.

#[path = "mock_idp/mod.rs"]
mod mock_idp;

use std::net::SocketAddr;

use hermes_hub::app::{Hub, Settings};
use hermes_hub::database::Repositories;
use mock_idp::MockIdp;
use reqwest::{Client, Method, Response, StatusCode};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

struct Rig {
    base: String,
    addr: SocketAddr,
    /// the admin
    admin: Client,
    idp: MockIdp,
    _web: tempfile::TempDir,
}

fn browser() -> Client {
    Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

impl Rig {
    async fn start() -> Self {
        Self::start_at("").await
    }

    async fn start_at(base_path: &str) -> Self {
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("index.html"), "<h1>hub</h1>").unwrap();
        let hub = Hub::new(
            Repositories::sqlite_in_memory().unwrap(),
            Settings {
                web: web.path().into(),
                base_path: base_path.into(),
                public_url: None,
                agent_image: "registry.example.com/agent:1.2.3".into(),
                agent_image_windows: String::new(),
                agent_pull_secret: String::new(),
                tls_enabled: false,
                federation_ca: None,
            },
        );
        hub.spawn_background();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(
            axum::serve(
                listener,
                hub.router()
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .into_future(),
        );
        let rig = Self {
            base: format!("http://{addr}{base_path}"),
            addr,
            admin: browser(),
            idp: mock_idp::start().await,
            _web: web,
        };
        let r = rig
            .call(
                &rig.admin,
                Method::POST,
                "/api/auth/setup",
                json!({"username": "admin", "password": PASSWORD}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
        rig
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn call(&self, who: &Client, method: Method, path: &str, body: Value) -> Response {
        who.request(method, self.url(path))
            .header("X-Requested-With", "hermes")
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn admin_json(&self, method: Method, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.call(&self.admin, method, path, body).await;
        (r.status(), r.json().await.unwrap_or(Value::Null))
    }

    /// Sets up a provider pointing at the stand-in; `extra` is merged over what a plain one has.
    async fn provider(&self, extra: Value) -> String {
        let mut body = json!({
            "name": "Mock IdP", "issuer": self.idp.issuer, "clientId": "hermes-client", "secret": "s3cret",
            "enabled": true, "autoCreate": true, "linkExisting": true,
        });
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, view) = self
            .admin_json(Method::POST, "/api/identity-providers", body)
            .await;
        assert_eq!(status, StatusCode::CREATED, "{view}");
        view["id"].as_str().unwrap().to_string()
    }

    /// The whole sign-in of a person in a fresh browser: to the provider and back. Returns the browser and the answer of the callback.
    async fn sign_in(&self, provider: &str, claims: Value) -> (Client, Response) {
        let b = browser();
        let start = b
            .get(self.url(&format!("/api/auth/sso/{provider}/login")))
            .send()
            .await
            .unwrap();
        assert_eq!(start.status(), StatusCode::FOUND);
        let to = start.headers()["location"].to_str().unwrap().to_string();
        assert!(
            to.starts_with(&self.idp.issuer),
            "sent to the provider: {to}"
        );
        let back = self.idp.sign_in(&to, claims);
        let done = self.callback(&b, provider, &back).await;
        (b, done)
    }

    async fn callback(&self, b: &Client, provider: &str, back: &mock_idp::Back) -> Response {
        let url = format!(
            "{}/api/auth/sso/{provider}/callback?code={}&state={}",
            self.base, back.code, back.state
        );
        b.get(url).send().await.unwrap()
    }

    async fn who(&self, b: &Client) -> Value {
        b.get(self.url("/api/auth/status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn users(&self) -> Value {
        self.admin_json(Method::GET, "/api/users", json!({}))
            .await
            .1
    }
}

/// Where the callback sent the browser, and whether it is in.
fn landed(r: &Response) -> String {
    assert_eq!(r.status(), StatusCode::FOUND);
    r.headers()["location"].to_str().unwrap().to_string()
}

fn error_of(r: &Response) -> String {
    let to = landed(r);
    let (_, query) = to
        .split_once("sso_error=")
        .unwrap_or_else(|| panic!("not an error: {to}"));
    url::form_urlencoded::parse(
        format!("sso_error={}", query.split('#').next().unwrap()).as_bytes(),
    )
    .next()
    .unwrap()
    .1
    .into_owned()
}

fn user_named<'a>(users: &'a Value, name: &str) -> &'a Value {
    users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == name)
        .unwrap_or_else(|| panic!("no {name} in {users}"))
}

#[tokio::test]
async fn a_person_signs_in_through_the_provider_and_gets_an_account_with_no_password() {
    let rig = Rig::start().await;
    let id = rig.provider(json!({})).await;

    // the login screen is told what to offer, to somebody who is not logged in
    let offered = rig.who(&browser()).await["sso"].clone();
    assert_eq!(offered, json!([{"id": id, "name": "Mock IdP"}]));

    let (b, done) = rig
        .sign_in(
            &id,
            json!({"sub": "u-1", "email": "ann@example.com", "preferred_username": "Ann.Lee"}),
        )
        .await;
    assert_eq!(landed(&done), "/#/admin/dashboard");
    let me = rig.who(&b).await;
    assert_eq!(
        (me["authenticated"].clone(), me["username"].clone()),
        (json!(true), json!("ann.lee"))
    );
    assert_eq!(me["grants"], json!([]), "in no group yet: nothing allowed");

    let users = rig.users().await;
    let ann = user_named(&users, "ann.lee");
    assert_eq!(
        (ann["sso"].clone(), ann["email"].clone()),
        (json!(id), json!("ann@example.com"))
    );

    // a second sign-in is the same account
    let (_, again) = rig
        .sign_in(&id, json!({"sub": "u-1", "email": "ann@example.com"}))
        .await;
    assert_eq!(landed(&again), "/#/admin/dashboard");
    assert_eq!(rig.users().await.as_array().unwrap().len(), 2);

    // there is no password to log in with, whatever is typed
    for pw in ["", "anything at all, long enough", PASSWORD] {
        let r = rig
            .call(
                &browser(),
                Method::POST,
                "/api/auth/login",
                json!({"username": "ann.lee", "password": pw}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
    // and the admin cannot give it one
    let r = rig
        .call(
            &rig.admin,
            Method::PATCH,
            &format!("/api/users/{}", ann["id"]),
            json!({"password": "a brand new password"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn nothing_that_is_not_a_sign_in_this_hub_started_gets_anyone_in() {
    let rig = Rig::start().await;
    let id = rig.provider(json!({})).await;
    let claims = || json!({"sub": "u-2", "email": "eve@example.com"});
    let begin = |b: &Client| {
        let b = b.clone();
        let url = rig.url(&format!("/api/auth/sso/{id}/login"));
        async move {
            b.get(url).send().await.unwrap().headers()["location"]
                .to_str()
                .unwrap()
                .to_string()
        }
    };

    // the browser that comes back is not the one that left: no cookie, or somebody else's
    let b = browser();
    let to = begin(&b).await;
    let back = rig.idp.sign_in(&to, claims());
    assert!(error_of(&rig.callback(&browser(), &id, &back).await).contains("did not start here"));
    // the code is still good for the browser that left, once
    assert_eq!(
        landed(&rig.callback(&b, &id, &back).await),
        "/#/admin/dashboard"
    );
    let replay = rig.callback(&b, &id, &back).await;
    assert!(
        error_of(&replay).contains("did not start here"),
        "a sign-in is used once"
    );

    // a made-up state
    let b = browser();
    begin(&b).await;
    let fake = mock_idp::Back {
        code: "code-x".into(),
        state: "made-up".into(),
    };
    assert!(error_of(&rig.callback(&b, &id, &fake).await).contains("did not start here"));

    // id tokens that are not right: signed by a key the provider never published, for another client, expired, for another sign-in
    for (what, claims, forged) in [
        ("a forged signature", claims(), true),
        (
            "another client",
            json!({"sub": "u-2", "aud": "someone-else"}),
            false,
        ),
        ("an expired token", json!({"sub": "u-2", "exp": 1}), false),
        (
            "another nonce",
            json!({"sub": "u-2", "nonce": "not-the-one"}),
            false,
        ),
        (
            "another issuer",
            json!({"sub": "u-2", "iss": "https://evil.example.com"}),
            false,
        ),
    ] {
        let b = browser();
        let to = begin(&b).await;
        let back = if forged {
            rig.idp.sign_in_forged(&to, claims)
        } else {
            rig.idp.sign_in(&to, claims)
        };
        let r = rig.callback(&b, &id, &back).await;
        assert!(
            error_of(&r).contains("id token is not valid"),
            "{what}: {}",
            landed(&r)
        );
        assert_eq!(rig.who(&b).await["authenticated"], false, "{what}");
    }
    // the provider turning the person away is told as it is
    let b = browser();
    begin(&b).await;
    let denied = b
        .get(rig.url(&format!(
            "/api/auth/sso/{id}/callback?error=access_denied&error_description=no%20thanks"
        )))
        .send()
        .await
        .unwrap();
    assert!(error_of(&denied).contains("no thanks"));
    // a provider that is switched off, or unknown, cannot be started
    assert!(
        error_of(
            &browser()
                .get(rig.url("/api/auth/sso/nope/login"))
                .send()
                .await
                .unwrap()
        )
        .contains("not found")
    );
}

#[tokio::test]
async fn a_local_account_with_the_same_email_becomes_theirs_and_keeps_its_groups() {
    let rig = Rig::start().await;
    let made = rig
        .call(
            &rig.admin,
            Method::POST,
            "/api/users",
            json!({"username": "bobby", "password": PASSWORD, "email": "Bob@Example.com"}),
        )
        .await;
    let bob_id = made.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
    let groups = rig
        .admin_json(Method::GET, "/api/groups", json!({}))
        .await
        .1;
    let viewers = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "viewers")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    rig.call(
        &rig.admin,
        Method::PUT,
        &format!("/api/groups/{viewers}/members"),
        json!({"users": [bob_id]}),
    )
    .await;
    let id = rig.provider(json!({"trustEmail": true})).await;

    let (b, done) = rig
        .sign_in(&id, json!({"sub": "entra-bob", "email": "bob@example.com"}))
        .await;
    assert_eq!(landed(&done), "/#/admin/dashboard");
    assert_eq!(
        rig.who(&b).await["username"],
        "bobby",
        "the same account, not a new one"
    );
    let users = rig.users().await;
    let bob = user_named(&users, "bobby");
    assert_eq!(
        (
            bob["id"].as_i64().unwrap(),
            bob["sso"].clone(),
            bob["groups"].as_array().unwrap().len()
        ),
        (bob_id, json!(id), 1)
    );
    assert_eq!(users.as_array().unwrap().len(), 2, "nobody new");
    // its password is gone
    let r = rig
        .call(
            &browser(),
            Method::POST,
            "/api/auth/login",
            json!({"username": "bobby", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // and what viewers gives, it gives
    assert_eq!(
        b.get(rig.url("/api/sources"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_email_the_provider_does_not_vouch_for_takes_nobodys_account() {
    let rig = Rig::start().await;
    rig.call(
        &rig.admin,
        Method::POST,
        "/api/users",
        json!({"username": "carol", "password": PASSWORD, "email": "carol@example.com"}),
    )
    .await;
    let id = rig.provider(json!({})).await; // not trusted: only email_verified counts
    let (_, done) = rig
        .sign_in(
            &id,
            json!({"sub": "x-1", "email": "carol@example.com", "preferred_username": "carol"}),
        )
        .await;
    assert_eq!(landed(&done), "/#/admin/dashboard");
    let users = rig.users().await;
    assert_eq!(
        user_named(&users, "carol")["sso"],
        "",
        "carol is still local"
    );
    assert_eq!(
        user_named(&users, "carol-2")["sso"],
        json!(id),
        "the newcomer got another name"
    );
    // vouched for: it counts
    let (_, done) = rig
        .sign_in(
            &id,
            json!({"sub": "x-2", "email": "carol@example.com", "email_verified": true}),
        )
        .await;
    assert_eq!(landed(&done), "/#/admin/dashboard");
    assert_eq!(user_named(&rig.users().await, "carol")["sso"], json!(id));
}

#[tokio::test]
async fn a_break_glass_account_is_never_taken_over() {
    let rig = Rig::start().await;
    let users = rig.users().await;
    let admin_id = user_named(&users, "admin")["id"].as_i64().unwrap();
    rig.call(
        &rig.admin,
        Method::PATCH,
        &format!("/api/users/{admin_id}"),
        json!({"email": "root@example.com"}),
    )
    .await;
    let id = rig.provider(json!({"trustEmail": true})).await;
    let (_, done) = rig
        .sign_in(
            &id,
            json!({"sub": "r-1", "email": "root@example.com", "preferred_username": "root"}),
        )
        .await;
    assert_eq!(landed(&done), "/#/admin/dashboard");
    let users = rig.users().await;
    assert_eq!(user_named(&users, "admin")["sso"], "");
    assert_eq!(user_named(&users, "root")["sso"], json!(id));
    // and an account cannot be made break-glass once it is a provider's
    let root_id = user_named(&users, "root")["id"].as_i64().unwrap();
    let r = rig
        .call(
            &rig.admin,
            Method::PATCH,
            &format!("/api/users/{root_id}"),
            json!({"breakGlass": true}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_provider_can_be_limited_to_some_domains_and_to_people_who_already_have_an_account() {
    let rig = Rig::start().await;
    let id = rig
        .provider(json!({"allowedDomains": "Example.com, corp.example.org", "autoCreate": false}))
        .await;
    let (_, r) = rig
        .sign_in(&id, json!({"sub": "d-1", "email": "x@gmail.com"}))
        .await;
    assert!(error_of(&r).contains("not from a domain allowed"));
    let (_, r) = rig.sign_in(&id, json!({"sub": "d-2"})).await;
    assert!(
        error_of(&r).contains("not from a domain allowed"),
        "no email at all, when domains are set"
    );
    let (_, r) = rig
        .sign_in(&id, json!({"sub": "d-3", "email": "x@example.com"}))
        .await;
    assert!(
        error_of(&r).contains("no account for you"),
        "from an allowed domain, but accounts are not made on sign-in"
    );
    assert_eq!(rig.users().await.as_array().unwrap().len(), 1);
    // a local account to link to makes it work
    rig.call(
        &rig.admin,
        Method::POST,
        "/api/users",
        json!({"username": "xavier", "password": PASSWORD, "email": "x@corp.example.org"}),
    )
    .await;
    let (_, r) = rig
        .sign_in(
            &id,
            json!({"sub": "d-4", "email": "x@corp.example.org", "email_verified": true}),
        )
        .await;
    assert_eq!(landed(&r), "/#/admin/dashboard");
}

#[tokio::test]
async fn the_groups_a_provider_says_are_the_groups_a_person_is_in_at_every_sign_in() {
    let rig = Rig::start().await;
    let groups = rig
        .admin_json(Method::GET, "/api/groups", json!({}))
        .await
        .1;
    let owners = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "owners")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let viewers = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "viewers")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let id = rig
        .provider(json!({"groupsClaim": "roles", "defaultGroup": viewers, "groupMap": [{"claim": "hub-admins", "group": owners}]}))
        .await;
    let can_write = |b: &Client| {
        let b = b.clone();
        let url = rig.url("/api/policies");
        async move {
            b.post(url)
                .header("X-Requested-With", "hermes")
                .json(&json!({"name": "x", "hcl": ""}))
                .send()
                .await
                .unwrap()
                .status()
        }
    };

    let (b, _) = rig
        .sign_in(
            &id,
            json!({"sub": "g-1", "preferred_username": "gina", "roles": ["hub-admins", "other"]}),
        )
        .await;
    assert_eq!(
        can_write(&b).await,
        StatusCode::CREATED,
        "in owners, which gives admin"
    );
    let users = rig.users().await;
    let gina = user_named(&users, "gina");
    assert_eq!(
        gina["groups"].as_array().unwrap().len(),
        2,
        "owners from the claim, viewers by default"
    );

    // no longer an admin at the provider: no longer one here, at the next sign-in. viewers (not mapped) stays
    let (b, _) = rig.sign_in(&id, json!({"sub": "g-1", "roles": []})).await;
    assert_eq!(can_write(&b).await, StatusCode::FORBIDDEN);
    assert_eq!(
        b.get(rig.url("/api/sources"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK,
        "viewers is still theirs"
    );

    // a token that has no such claim at all says nothing: nothing is taken away
    let (_, _) = rig
        .sign_in(&id, json!({"sub": "g-1", "roles": ["hub-admins"]}))
        .await;
    let (b, _) = rig.sign_in(&id, json!({"sub": "g-1"})).await;
    assert_ne!(can_write(&b).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_disabled_account_is_turned_away_and_a_provider_that_insists_leaves_passwords_to_break_glass()
 {
    let rig = Rig::start().await;
    let id = rig.provider(json!({})).await;
    let (_, _) = rig
        .sign_in(&id, json!({"sub": "h-1", "preferred_username": "hank"}))
        .await;
    let users = rig.users().await;
    let hank = user_named(&users, "hank")["id"].as_i64().unwrap();
    rig.call(
        &rig.admin,
        Method::PATCH,
        &format!("/api/users/{hank}"),
        json!({"disabled": true}),
    )
    .await;
    let (b, r) = rig.sign_in(&id, json!({"sub": "h-1"})).await;
    assert!(error_of(&r).contains("disabled"));
    assert_eq!(rig.who(&b).await["authenticated"], false);

    // insisting: a local account can no longer log in with its password, the break-glass one still can
    rig.call(
        &rig.admin,
        Method::POST,
        "/api/users",
        json!({"username": "ivan", "password": PASSWORD}),
    )
    .await;
    let login = |name: &'static str| {
        let url = rig.url("/api/auth/login");
        async move {
            browser()
                .post(url)
                .header("X-Requested-With", "hermes")
                .json(&json!({"username": name, "password": PASSWORD}))
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(login("ivan").await, StatusCode::OK);
    let (_, view) = rig
        .admin_json(Method::GET, "/api/identity-providers", json!({}))
        .await;
    let mut input = view[0].clone();
    input["enforce"] = json!(true);
    let (status, _) = rig
        .admin_json(
            Method::PUT,
            &format!("/api/identity-providers/{id}"),
            input.clone(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login("ivan").await, StatusCode::UNAUTHORIZED);
    assert_eq!(login("admin").await, StatusCode::OK);
    // switched off, it insists on nothing
    input["enforce"] = json!(true);
    input["enabled"] = json!(false);
    rig.admin_json(Method::PUT, &format!("/api/identity-providers/{id}"), input)
        .await;
    assert_eq!(login("ivan").await, StatusCode::OK);
}

#[tokio::test]
async fn the_admin_sets_providers_up_and_the_secret_never_comes_back() {
    let rig = Rig::start().await;
    let id = rig.provider(json!({"scopes": "", "groupsClaim": ""})).await;
    let (_, list) = rig
        .admin_json(Method::GET, "/api/identity-providers", json!({}))
        .await;
    let p = &list[0];
    assert_eq!(
        (
            p["hasSecret"].clone(),
            p["scopes"].clone(),
            p["groupsClaim"].clone()
        ),
        (json!(true), json!("openid profile email"), json!("groups"))
    );
    assert!(!list.to_string().contains("s3cret"), "{list}");
    assert_eq!(
        p["redirectUri"],
        format!("http://{}/api/auth/sso/{id}/callback", rig.addr)
    );

    // a change without a secret keeps the one that is saved
    let mut input = p.clone();
    input["name"] = json!("Renamed");
    let (status, _) = rig
        .admin_json(Method::PUT, &format!("/api/identity-providers/{id}"), input)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (b, r) = rig
        .sign_in(&id, json!({"sub": "s-1", "preferred_username": "sara"}))
        .await;
    assert_eq!(
        landed(&r),
        "/#/admin/dashboard",
        "the saved secret still works"
    );
    assert_eq!(rig.who(&b).await["authenticated"], true);

    // what is refused, and why
    for (bad, why) in [
        (json!({"issuer": "http://idp.example.com"}), "https://"),
        (json!({"issuer": "not a url"}), "https://"),
        (json!({"name": " "}), "name"),
        (json!({"clientId": ""}), "client id"),
        (json!({"defaultGroup": 9999}), "default group"),
        (
            json!({"groupMap": [{"claim": "a", "group": 9999}]}),
            "mapped group",
        ),
        (json!({"allowedDomains": "not-a-domain"}), "domains"),
    ] {
        let mut body = json!({"name": "X", "issuer": rig.idp.issuer, "clientId": "c"});
        for (k, v) in bad.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, answer) = rig
            .admin_json(Method::POST, "/api/identity-providers", body)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert!(
            answer["error"].as_str().unwrap().contains(why),
            "{bad}: {answer}"
        );
    }
    // testing an address: found, or what is wrong
    let (_, ok) = rig
        .admin_json(
            Method::POST,
            "/api/identity-providers/test",
            json!({"issuer": rig.idp.issuer}),
        )
        .await;
    assert_eq!(ok["ok"], true);
    assert!(ok["message"].as_str().unwrap().contains("found"), "{ok}");
    let (_, no) = rig
        .admin_json(
            Method::POST,
            "/api/identity-providers/test",
            json!({"issuer": "http://127.0.0.1:1"}),
        )
        .await;
    assert_eq!(no["ok"], false);

    // switched off or deleted: nobody can start a sign-in with it, and the people who came through it stay as they are
    rig.admin_json(
        Method::DELETE,
        &format!("/api/identity-providers/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(rig.who(&browser()).await.get("sso"), None);
    assert_eq!(user_named(&rig.users().await, "sara")["sso"], json!(id));
    assert_eq!(
        rig.admin_json(
            Method::DELETE,
            &format!("/api/identity-providers/{id}"),
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn under_a_base_path_the_callback_address_has_the_path_and_the_sign_in_works() {
    let rig = Rig::start_at("/hermes").await;
    let id = rig.provider(json!({})).await;
    let (_, list) = rig
        .admin_json(Method::GET, "/api/identity-providers", json!({}))
        .await;
    assert_eq!(
        list[0]["redirectUri"],
        format!("http://{}/hermes/api/auth/sso/{id}/callback", rig.addr)
    );
    let (b, done) = rig
        .sign_in(&id, json!({"sub": "p-1", "preferred_username": "pat"}))
        .await;
    assert_eq!(landed(&done), "/hermes/#/admin/dashboard");
    assert_eq!(rig.who(&b).await["username"], "pat");
}

#[tokio::test]
async fn only_an_admin_with_the_right_may_look_at_the_providers() {
    let rig = Rig::start().await;
    rig.provider(json!({})).await;
    rig.call(
        &rig.admin,
        Method::POST,
        "/api/users",
        json!({"username": "nobody", "password": PASSWORD}),
    )
    .await;
    let b = browser();
    let r = rig
        .call(
            &b,
            Method::POST,
            "/api/auth/login",
            json!({"username": "nobody", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        rig.call(&b, Method::GET, "/api/identity-providers", json!({}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rig.call(
            &browser(),
            Method::GET,
            "/api/identity-providers",
            json!({})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}
