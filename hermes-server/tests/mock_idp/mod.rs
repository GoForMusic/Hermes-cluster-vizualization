//! A stand-in for an OpenID Connect identity provider (what Microsoft Entra ID, Keycloak, Google are, to the hub): discovery, the keys, the
//! authorize address and the token endpoint, with the id token signed with a key made for the test run. It checks what a real provider
//! checks (the PKCE verifier against the challenge, the code used once), so a hub that skips it fails here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Key {
    encoding: EncodingKey,
    n: String,
    e: String,
}

fn make_key() -> Key {
    let private = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    let public = RsaPublicKey::from(&private);
    Key {
        encoding: EncodingKey::from_rsa_der(private.to_pkcs1_der().unwrap().as_bytes()),
        n: URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
        e: URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
    }
}

/// One key for the whole test run, and another that no provider ever published.
fn keys() -> &'static (Key, Key) {
    static KEYS: OnceLock<(Key, Key)> = OnceLock::new();
    KEYS.get_or_init(|| (make_key(), make_key()))
}

struct Granted {
    claims: Value,
    nonce: String,
    challenge: String,
    client_id: String,
    redirect_uri: String,
    sign_with_unknown_key: bool,
}

#[derive(Default)]
struct State_ {
    codes: HashMap<String, Granted>,
    issued: u32,
}

pub struct MockIdp {
    pub issuer: String,
    state: Arc<Mutex<State_>>,
}

/// What the hub's browser would be sent back to.
pub struct Back {
    pub code: String,
    pub state: String,
}

type Shared = (String, Arc<Mutex<State_>>);

pub async fn start() -> MockIdp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State_::default()));
    let shared: Shared = (issuer.clone(), state.clone());
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(shared);
    tokio::spawn(axum::serve(listener, app).into_future());
    MockIdp { issuer, state }
}

async fn discovery(State((issuer, _)): State<Shared>) -> Json<Value> {
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks() -> Json<Value> {
    let k = &keys().0;
    Json(
        json!({"keys": [{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "test-key", "n": k.n, "e": k.e}]}),
    )
}

async fn token(
    State((issuer, state)): State<Shared>,
    Form(form): Form<HashMap<String, String>>,
) -> (axum::http::StatusCode, Json<Value>) {
    let bad = |why: &str| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": why})),
        )
    };
    let mut st = state.lock().unwrap();
    // a code works once, like a real one
    let Some(granted) = form.get("code").and_then(|c| st.codes.remove(c)) else {
        return bad("unknown or used code");
    };
    let verifier = form.get("code_verifier").map_or("", String::as_str);
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != granted.challenge {
        return bad("PKCE verifier does not match the challenge");
    }
    if form.get("redirect_uri") != Some(&granted.redirect_uri) {
        return bad("redirect_uri differs");
    }
    st.issued += 1;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut claims = json!({"iss": issuer, "aud": granted.client_id, "iat": now, "exp": now + 300, "nonce": granted.nonce});
    for (k, v) in granted.claims.as_object().unwrap() {
        claims[k] = v.clone();
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    let key = if granted.sign_with_unknown_key {
        &keys().1
    } else {
        &keys().0
    };
    let id_token = jsonwebtoken::encode(&header, &claims, &key.encoding).unwrap();
    (
        axum::http::StatusCode::OK,
        Json(
            json!({"access_token": "not-used", "token_type": "Bearer", "expires_in": 300, "id_token": id_token}),
        ),
    )
}

impl MockIdp {
    /// The person signs in at the provider as `claims` (`sub`, `email`, `groups`, ...; `iss`, `aud`, `exp` and `nonce` are filled in, and
    /// anything given here wins over them). `authorize_url` is where the hub sent the browser.
    pub fn sign_in(&self, authorize_url: &str, claims: Value) -> Back {
        self.grant(authorize_url, claims, false)
    }

    /// Like `sign_in`, with an id token signed by a key the provider never published.
    pub fn sign_in_forged(&self, authorize_url: &str, claims: Value) -> Back {
        self.grant(authorize_url, claims, true)
    }

    fn grant(&self, authorize_url: &str, claims: Value, forged: bool) -> Back {
        let url = url::Url::parse(authorize_url).unwrap();
        assert_eq!(url.path(), "/authorize", "{authorize_url}");
        let q: HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256", "the hub must use PKCE");
        assert!(
            q["scope"].split(' ').any(|s| s == "openid"),
            "{}",
            q["scope"]
        );
        let mut st = self.state.lock().unwrap();
        let code = format!("code-{}", st.codes.len() + st.issued as usize + 1);
        st.codes.insert(
            code.clone(),
            Granted {
                claims,
                nonce: q["nonce"].clone(),
                challenge: q["code_challenge"].clone(),
                client_id: q["client_id"].clone(),
                redirect_uri: q["redirect_uri"].clone(),
                sign_with_unknown_key: forged,
            },
        );
        Back {
            code,
            state: q["state"].clone(),
        }
    }
}
