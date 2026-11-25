//! Single sign-on with OpenID Connect: the identity providers an admin sets up (Microsoft Entra ID, Google, Keycloak, Okta, ...), the login
//! with one of them (authorization code with PKCE; the id token is checked for its signature, issuer, audience, expiry and nonce by
//! `openidconnect`), and what a successful login becomes here: an account, put in the groups the provider says it belongs to.
//!
//! The provider only says who a person is. What they may do is still decided here, by the policies of their groups (`policy.rs`).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde_json::Value;
use tracing::{info, warn};

use crate::database::{IGroupDAO, IIdpDAO, IUserDAO, NewUser, UserPatch, now_ms};
use crate::model::{GroupMapping, Provider, ProviderInput, ProviderView, SsoProviderView, User};
use crate::services::IAuthz;

/// How long a person has to come back from the provider's sign-in page.
const PENDING_FOR: Duration = Duration::from_secs(10 * 60);
const PENDING_MAX: usize = 2000;

#[derive(Debug)]
pub enum SsoError {
    /// Something the admin sent: what is wrong.
    Invalid(String),
    NotFound,
    /// The provider could not be reached, or answered something that is not OpenID Connect.
    Provider(String),
    /// The person came back with something that does not belong to a sign-in this hub started.
    BadState,
    /// What the person is told when they may not come in.
    Refused(String),
    Internal(anyhow::Error),
}

impl fmt::Display for SsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) | Self::Provider(why) | Self::Refused(why) => f.write_str(why),
            Self::NotFound => f.write_str("not found"),
            Self::BadState => {
                f.write_str("this sign-in did not start here, or took too long: try again")
            }
            Self::Internal(_) => f.write_str("internal error"),
        }
    }
}

impl From<anyhow::Error> for SsoError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

/// The address to send the person to, and what ties their return to it.
pub struct Begin {
    pub url: String,
    pub state: String,
}

struct Pending {
    provider: String,
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    redirect_uri: String,
    at: Instant,
}

/// What the provider said about the person.
struct Identity {
    subject: String,
    email: Option<String>,
    email_verified: Option<bool>,
    username: String,
    /// `None`: the token has no groups claim at all (do not touch their groups).
    groups: Option<Vec<String>>,
}

pub struct SsoImp {
    idps: Arc<dyn IIdpDAO>,
    users: Arc<dyn IUserDAO>,
    groups: Arc<dyn IGroupDAO>,
    authz: Arc<dyn IAuthz>,
    http: reqwest::Client,
    pending: Mutex<HashMap<String, Pending>>,
    write: Mutex<()>,
}

impl SsoImp {
    pub fn new(
        idps: Arc<dyn IIdpDAO>,
        users: Arc<dyn IUserDAO>,
        groups: Arc<dyn IGroupDAO>,
        authz: Arc<dyn IAuthz>,
    ) -> Self {
        let http = reqwest::ClientBuilder::new()
            // a provider that redirects is an SSRF waiting to happen
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .expect("an HTTP client with these settings");
        Self {
            idps,
            users,
            groups,
            authz,
            http,
            pending: Mutex::default(),
            write: Mutex::default(),
        }
    }

    // ---- what the login screen shows ---------------------------------------------------------------------------

    pub fn offered(&self) -> Vec<SsoProviderView> {
        self.idps
            .list_providers()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.enabled)
            .map(|p| SsoProviderView {
                id: p.id,
                name: p.name,
            })
            .collect()
    }

    // ---- the admin's side --------------------------------------------------------------------------------------

    /// `redirect_uri(id)`: the callback address of this hub for a provider, as the admin must register it there.
    pub fn views(
        &self,
        redirect_uri: &dyn Fn(&str) -> String,
    ) -> Result<Vec<ProviderView>, SsoError> {
        self.idps
            .list_providers()?
            .iter()
            .map(|p| self.view(p, redirect_uri(&p.id)))
            .collect()
    }

    fn view(&self, p: &Provider, redirect_uri: String) -> Result<ProviderView, SsoError> {
        Ok(ProviderView {
            id: p.id.clone(),
            name: p.name.clone(),
            issuer: p.issuer.clone(),
            client_id: p.client_id.clone(),
            has_secret: !p.client_secret.is_empty(),
            scopes: p.scopes.clone(),
            groups_claim: p.groups_claim.clone(),
            allowed_domains: p.allowed_domains.clone(),
            enabled: p.enabled,
            auto_create: p.auto_create,
            link_existing: p.link_existing,
            trust_email: p.trust_email,
            enforce: p.enforce,
            default_group: p.default_group,
            group_map: self
                .idps
                .group_map(&p.id)?
                .into_iter()
                .map(|(claim, group)| GroupMapping { claim, group })
                .collect(),
            redirect_uri,
        })
    }

    pub fn create(
        &self,
        input: &ProviderInput,
        redirect_uri: &dyn Fn(&str) -> String,
    ) -> Result<ProviderView, SsoError> {
        let _one = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut p = self.checked(input, None)?;
        let existing: HashSet<String> = self
            .idps
            .list_providers()?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let base = slug(&input.name);
        p.id = (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                }
            })
            .find(|id| !existing.contains(id))
            .unwrap_or(base);
        p.created = now_ms();
        self.idps.create_provider(&p)?;
        self.idps.set_group_map(&p.id, &pairs(input))?;
        self.view(&p, redirect_uri(&p.id))
    }

    pub fn update(
        &self,
        id: &str,
        input: &ProviderInput,
        redirect_uri: &dyn Fn(&str) -> String,
    ) -> Result<ProviderView, SsoError> {
        let _one = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let old = self.idps.provider(id)?.ok_or(SsoError::NotFound)?;
        let mut p = self.checked(input, Some(&old))?;
        p.id = old.id;
        p.created = old.created;
        self.idps.update_provider(&p)?;
        self.idps.set_group_map(&p.id, &pairs(input))?;
        self.view(&p, redirect_uri(&p.id))
    }

    pub fn delete(&self, id: &str) -> Result<(), SsoError> {
        let _one = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        self.idps.provider(id)?.ok_or(SsoError::NotFound)?;
        Ok(self.idps.delete_provider(id)?)
    }

    fn checked(&self, input: &ProviderInput, old: Option<&Provider>) -> Result<Provider, SsoError> {
        let invalid = |why: &str| Err(SsoError::Invalid(why.to_string()));
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.contains(['\n', '\r']) {
            return invalid("a provider's name is 1-64 characters");
        }
        let issuer = input.issuer.trim().trim_end_matches('/').to_string();
        if !issuer_ok(&issuer) {
            return invalid(
                "the issuer is the https:// address of the provider (http:// only for localhost)",
            );
        }
        if input.client_id.trim().is_empty() {
            return invalid("the client id is what the provider gave this hub");
        }
        let all_groups: HashSet<i64> = self
            .groups
            .list_groups()?
            .into_iter()
            .map(|g| g.id)
            .collect();
        if input
            .default_group
            .is_some_and(|g| !all_groups.contains(&g))
        {
            return invalid("the default group does not exist");
        }
        let mut seen = HashSet::new();
        for m in &input.group_map {
            if m.claim.trim().is_empty() || !seen.insert(m.claim.trim().to_string()) {
                return invalid("each value of the groups claim is mapped once, and none is empty");
            }
            if !all_groups.contains(&m.group) {
                return invalid("a mapped group does not exist");
            }
        }
        let domains = input
            .allowed_domains
            .split([',', ' ', '\n'])
            .map(|d| d.trim().trim_start_matches('@').to_lowercase())
            .filter(|d| !d.is_empty())
            .collect::<Vec<_>>();
        if domains
            .iter()
            .any(|d| !d.contains('.') || d.contains(['/', '@', ' ']))
        {
            return invalid("allowed domains look like example.com, separated by commas");
        }
        let scopes = input.scopes.trim();
        let groups_claim = input.groups_claim.trim();
        Ok(Provider {
            name: name.to_string(),
            issuer,
            client_id: input.client_id.trim().to_string(),
            client_secret: if input.secret.is_empty() {
                old.map(|o| o.client_secret.clone()).unwrap_or_default()
            } else {
                input.secret.clone()
            },
            scopes: if scopes.is_empty() {
                "openid profile email".into()
            } else {
                scopes.to_string()
            },
            groups_claim: if groups_claim.is_empty() {
                "groups".into()
            } else {
                groups_claim.to_string()
            },
            allowed_domains: domains.join(","),
            enabled: input.enabled,
            auto_create: input.auto_create,
            link_existing: input.link_existing,
            trust_email: input.trust_email,
            enforce: input.enforce,
            default_group: input.default_group,
            ..Provider::default()
        })
    }

    /// Reads the provider's discovery document: the address is right, and it speaks OpenID Connect.
    pub async fn test(&self, issuer: &str) -> Result<String, SsoError> {
        let issuer = issuer.trim().trim_end_matches('/').to_string();
        if !issuer_ok(&issuer) {
            return Err(SsoError::Invalid(
                "the issuer is the https:// address of the provider (http:// only for localhost)"
                    .into(),
            ));
        }
        let metadata = self.discover(&issuer).await?;
        Ok(format!(
            "found {} (login at {})",
            metadata.issuer().as_str(),
            metadata.authorization_endpoint().as_str()
        ))
    }

    async fn discover(&self, issuer: &str) -> Result<CoreProviderMetadata, SsoError> {
        let url = IssuerUrl::new(issuer.to_string())
            .map_err(|e| SsoError::Invalid(format!("not an address: {e}")))?;
        CoreProviderMetadata::discover_async(url, &self.http)
            .await
            .map_err(|e| {
                SsoError::Provider(format!(
                    "cannot read {issuer}/.well-known/openid-configuration: {}",
                    chain(&e)
                ))
            })
    }

    // ---- the login ---------------------------------------------------------------------------------------------

    /// Starts a login with a provider: where to send the person. `redirect_uri` is this hub's callback address for it.
    pub async fn begin(&self, provider: &str, redirect_uri: &str) -> Result<Begin, SsoError> {
        let p = self.enabled(provider)?;
        let client = self.client(&p, redirect_uri).await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge);
        for scope in p
            .scopes
            .split([' ', ','])
            .filter(|s| !s.is_empty() && *s != "openid")
        {
            request = request.add_scope(Scope::new(scope.to_string()));
        }
        let (url, state, nonce) = request.url();
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.retain(|_, v| v.at.elapsed() < PENDING_FOR);
        if pending.len() >= PENDING_MAX {
            return Err(SsoError::Refused(
                "too many sign-ins at once: try again in a minute".into(),
            ));
        }
        pending.insert(
            state.secret().clone(),
            Pending {
                provider: p.id,
                nonce,
                verifier,
                redirect_uri: redirect_uri.to_string(),
                at: Instant::now(),
            },
        );
        Ok(Begin {
            url: url.to_string(),
            state: state.secret().clone(),
        })
    }

    /// The person is back with a code. Returns who they are here (the account is made or found, and put in their groups).
    pub async fn finish(&self, provider: &str, code: &str, state: &str) -> Result<User, SsoError> {
        let pending = {
            let mut map = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            map.remove(state)
        };
        let pending = pending
            .filter(|p| p.provider == provider && p.at.elapsed() < PENDING_FOR)
            .ok_or(SsoError::BadState)?;
        let p = self.enabled(provider)?;
        let client = self.client(&p, &pending.redirect_uri).await?;
        let token = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|e| SsoError::Provider(format!("the provider has no token endpoint: {e}")))?
            .set_pkce_verifier(pending.verifier)
            .request_async(&self.http)
            .await
            .map_err(|e| {
                SsoError::Provider(format!("the provider refused the code: {}", chain(&e)))
            })?;
        let id_token = token
            .id_token()
            .ok_or_else(|| SsoError::Provider("the provider sent no id token".into()))?;
        let claims = id_token
            .claims(&client.id_token_verifier(), &pending.nonce)
            .map_err(|e| SsoError::Provider(format!("the id token is not valid: {e}")))?;
        let jwt = serde_json::to_value(id_token)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let identity = Identity {
            subject: claims.subject().as_str().to_string(),
            email: claims
                .email()
                .map(|e| e.as_str().trim().to_string())
                .filter(|e| !e.is_empty()),
            email_verified: claims.email_verified(),
            username: claims
                .preferred_username()
                .map(|u| u.as_str().to_string())
                .unwrap_or_default(),
            groups: groups_of(&jwt, &p.groups_claim),
        };
        let user = self.admit(&p, &identity)?;
        self.authz.forget();
        Ok(user)
    }

    fn enabled(&self, id: &str) -> Result<Provider, SsoError> {
        self.idps
            .provider(id)?
            .filter(|p| p.enabled)
            .ok_or(SsoError::NotFound)
    }

    async fn client(
        &self,
        p: &Provider,
        redirect_uri: &str,
    ) -> Result<
        CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointMaybeSet,
            openidconnect::EndpointMaybeSet,
        >,
        SsoError,
    > {
        let metadata = self.discover(&p.issuer).await?;
        let secret =
            (!p.client_secret.is_empty()).then(|| ClientSecret::new(p.client_secret.clone()));
        Ok(
            CoreClient::from_provider_metadata(
                metadata,
                ClientId::new(p.client_id.clone()),
                secret,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_uri.to_string())
                    .map_err(|e| SsoError::Invalid(format!("not an address: {e}")))?,
            ),
        )
    }

    // ---- who they are here -------------------------------------------------------------------------------------

    fn admit(&self, p: &Provider, who: &Identity) -> Result<User, SsoError> {
        let _one = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let refused = |why: &str| SsoError::Refused(why.to_string());
        if !p.allowed_domains.is_empty() {
            let domain = who
                .email
                .as_deref()
                .and_then(|e| e.rsplit_once('@'))
                .map(|(_, d)| d.to_lowercase());
            if !domain.is_some_and(|d| p.allowed_domains.split(',').any(|a| a == d)) {
                return Err(refused(
                    "this email address is not from a domain allowed to sign in here",
                ));
            }
        }
        let mut user = match self.users.user_by_external(&p.id, &who.subject)? {
            Some(u) => u,
            None => self.link_or_create(p, who)?,
        };
        if user.disabled {
            return Err(refused("this account is disabled"));
        }
        if let Some(email) = &who.email
            && user.email.to_lowercase() != email.to_lowercase()
        {
            self.users.update_user(
                user.id,
                &UserPatch {
                    email: Some(email.clone()),
                    ..UserPatch::default()
                },
            )?;
            user.email.clone_from(email);
        }
        self.sync_groups(p, &user, who)?;
        info!("{} signed in through {}", user.username, p.name);
        Ok(user)
    }

    fn link_or_create(&self, p: &Provider, who: &Identity) -> Result<User, SsoError> {
        let refused = |why: &str| SsoError::Refused(why.to_string());
        // a local account with this email becomes theirs, when the provider vouches for the address
        if p.link_existing
            && let Some(email) = &who.email
            && (who.email_verified == Some(true) || p.trust_email)
        {
            let candidates: Vec<User> = self
                .users
                .local_users_by_email(email)?
                .into_iter()
                .filter(|u| !u.break_glass)
                .collect();
            match candidates.as_slice() {
                [] => {}
                [one] => {
                    self.users.link_user(one.id, &p.id, &who.subject)?;
                    info!(
                        "the local account {} is now {}'s sign-in through {}",
                        one.username, email, p.name
                    );
                    return self.users.user_by_id(one.id)?.ok_or(SsoError::NotFound);
                }
                _ => {
                    return Err(refused(
                        "more than one account here has this email address: an admin has to sort it out",
                    ));
                }
            }
        }
        if !p.auto_create {
            return Err(refused("there is no account for you here: ask an admin"));
        }
        let name = self.free_username(&who.username, who.email.as_deref())?;
        let user = self.users.create_user(&NewUser {
            username: &name,
            password_hash: "",
            role: "admin",
            email: who.email.as_deref().unwrap_or(""),
            break_glass: false,
            created: now_ms(),
            idp: &p.id,
            external_id: &who.subject,
        })?;
        if let Some(g) = p.default_group {
            self.groups.add_member(g, user.id)?;
        }
        Ok(user)
    }

    fn free_username(&self, preferred: &str, email: Option<&str>) -> Result<String, SsoError> {
        let hint = if preferred.is_empty() {
            email.map_or("", |e| e.split('@').next().unwrap_or(""))
        } else {
            preferred
        };
        let hint = hint.split('@').next().unwrap_or("");
        let mut base: String = hint
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
                    c
                } else {
                    '.'
                }
            })
            .collect();
        base = base.trim_matches('.').to_string();
        if base.len() < 3 {
            base = format!("sso-{base}").trim_end_matches('-').to_string();
        }
        base.truncate(26);
        for n in 1..1000 {
            let candidate = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            if self.users.user_by_name(&candidate)?.is_none() {
                return Ok(candidate);
            }
        }
        Err(SsoError::Refused(
            "no free username for this account".into(),
        ))
    }

    /// The groups the provider maps are exactly the ones its claim says, at every login. Others are not touched, and a token without the
    /// groups claim leaves everything as it is (it is not the same as being in no group).
    fn sync_groups(&self, p: &Provider, user: &User, who: &Identity) -> Result<(), SsoError> {
        let Some(claimed) = &who.groups else {
            return Ok(());
        };
        let map = self.idps.group_map(&p.id)?;
        if map.is_empty() {
            return Ok(());
        }
        let claimed: HashSet<&str> = claimed.iter().map(String::as_str).collect();
        let current: HashSet<i64> = self
            .groups
            .memberships()?
            .into_iter()
            .filter(|(_, u)| *u == user.id)
            .map(|(g, _)| g)
            .collect();
        let mapped: HashSet<i64> = map.iter().map(|(_, g)| *g).collect();
        for group in mapped {
            let wanted = map
                .iter()
                .any(|(c, g)| *g == group && claimed.contains(c.as_str()));
            match (wanted, current.contains(&group)) {
                (true, false) => self.groups.add_member(group, user.id)?,
                (false, true) => self.groups.remove_member(group, user.id)?,
                _ => {}
            }
        }
        Ok(())
    }
}

fn pairs(input: &ProviderInput) -> Vec<(String, i64)> {
    input
        .group_map
        .iter()
        .map(|m| (m.claim.trim().to_string(), m.group))
        .collect()
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "sso".into()
    } else {
        s.chars().take(40).collect()
    }
}

/// https, or http for a provider on this machine (a lab, a test).
fn issuer_ok(issuer: &str) -> bool {
    let Ok(url) = url::Url::parse(issuer) else {
        return false;
    };
    match (url.scheme(), url.host_str()) {
        ("https", Some(_)) => true,
        ("http", Some(host)) => matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1"),
        _ => false,
    }
}

/// The values of the groups claim in an id token that was already verified. (The claim is not one the library's standard claims have.)
fn groups_of(jwt: &str, claim: &str) -> Option<Vec<String>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json: Value = serde_json::from_slice(&bytes).ok()?;
    match json.get(claim)? {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        ),
        Value::String(one) => Some(vec![one.clone()]),
        _ => None,
    }
}

/// An error with what caused it (a library error alone says only "request failed").
fn chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    if text.len() > 300 {
        warn!("a long provider error: {text}");
        text.truncate(300);
    }
    text
}

#[cfg(test)]
#[path = "../../tests/unit/sso.rs"]
mod tests;
