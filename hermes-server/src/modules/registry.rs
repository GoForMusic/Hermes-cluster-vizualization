//! Talking to a container registry (Docker Hub, Harbor, Gitea, GHCR, a plain `registry:2`...): they all speak the OCI distribution
//! API, and differ only in how they log you in. Basic auth is tried first; a `Bearer` challenge is answered with a token from the
//! registry's own token service, the way `docker login` does it.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::Deserialize;

use crate::model::RegistryConfig;

/// What the registry sends when it wants a token: `Bearer realm="https://…/token",service="registry",scope="…"`.
#[derive(Debug, Default, PartialEq)]
struct Challenge {
    realm: String,
    service: String,
    scope: String,
}

fn parse_challenge(header: &str) -> Option<Challenge> {
    let rest = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))?;
    let mut c = Challenge::default();
    for part in split_params(rest) {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        match k.trim().to_ascii_lowercase().as_str() {
            "realm" => c.realm = v,
            "service" => c.service = v,
            "scope" => c.scope = v,
            _ => {}
        }
    }
    (!c.realm.is_empty()).then_some(c)
}

/// Splits on the commas that are outside quotes (a scope can hold one: `repository:a/b:pull,push`).
fn split_params(s: &str) -> Vec<&str> {
    let (mut out, mut start, mut quoted) = (Vec::new(), 0, false);
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

pub struct RegistryClient {
    http: Client,
}

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryClient {
    pub fn new() -> Self {
        // the TLS provider is process-wide; main() also installs it early (for the hub's own TLS, when it has one), so this
        // is a second, harmless install for anything (tests, mainly) that builds a client without going through main() first.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("the HTTP client builds with its default settings");
        Self { http }
    }

    /// `GET path` on the registry, logging in the way it asks for. `repo` is the repository the call is about (for the token's scope).
    async fn get(
        &self,
        config: &RegistryConfig,
        path: &str,
        repo: Option<&str>,
    ) -> Result<Response> {
        let url = format!("{}{path}", config.base());
        let basic = |req: RequestBuilder| {
            if config.auth == "basic" {
                req.basic_auth(&config.username, Some(&config.secret))
            } else {
                req
            }
        };
        let first = basic(self.http.get(&url))
            .send()
            .await
            .map_err(|e| unreachable(config, &e))?;
        if first.status() != StatusCode::UNAUTHORIZED {
            return Ok(first);
        }
        let challenge = first
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_challenge);
        let Some(challenge) = challenge else {
            return Ok(first); // a Basic challenge: the credentials were sent already, and refused
        };
        let token = self.token(config, &challenge, repo).await?;
        self.http
            .get(&url)
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| unreachable(config, &e))
    }

    async fn token(
        &self,
        config: &RegistryConfig,
        c: &Challenge,
        repo: Option<&str>,
    ) -> Result<String> {
        #[derive(Deserialize)]
        struct Token {
            token: Option<String>,
            access_token: Option<String>,
        }
        let mut req = self.http.get(&c.realm);
        if !c.service.is_empty() {
            req = req.query(&[("service", &c.service)]);
        }
        let scope = match repo {
            Some(repo) => format!("repository:{repo}:pull"),
            None => c.scope.clone(),
        };
        if !scope.is_empty() {
            req = req.query(&[("scope", &scope)]);
        }
        if config.auth == "basic" {
            req = req.basic_auth(&config.username, Some(&config.secret));
        }
        let res = req.send().await.map_err(|e| {
            anyhow!(
                "the registry's login service {} is not reachable: {e}",
                c.realm
            )
        })?;
        if !res.status().is_success() {
            bail!("the registry refused the login ({})", res.status());
        }
        let t: Token = res
            .json()
            .await
            .context("the registry's login service answered something unexpected")?;
        t.token
            .or(t.access_token)
            .ok_or_else(|| anyhow!("the registry's login service sent no token"))
    }

    /// Whether the registry answers and accepts these credentials.
    pub async fn check(&self, config: &RegistryConfig) -> Result<()> {
        if !config.is_set() {
            bail!("the registry address is empty");
        }
        let res = self.get(config, "/v2/", None).await?;
        match res.status() {
            s if s.is_success() => Ok(()),
            StatusCode::UNAUTHORIZED => bail!("{}", refused(config)),
            StatusCode::NOT_FOUND => bail!(
                "{} answers, but it is not a container registry (no /v2/)",
                config.host()
            ),
            s => bail!("the registry answered {s}"),
        }
    }

    /// The tags of one repository under the configured project, newest version first (tags that are not versions follow, alphabetically).
    pub async fn tags(&self, config: &RegistryConfig, repo: &str) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct List {
            tags: Option<Vec<String>>,
        }
        let full = full_repo(config, repo);
        let res = self
            .get(config, &format!("/v2/{full}/tags/list?n=1000"), Some(&full))
            .await?;
        match res.status() {
            s if s.is_success() => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => bail!("{}", refused(config)),
            StatusCode::NOT_FOUND => bail!(
                "{full} is not in {} (push the agent image there first)",
                config.host()
            ),
            s => bail!("the registry answered {s}"),
        }
        let list: List = res
            .json()
            .await
            .context("the registry's answer is not a tag list")?;
        Ok(sort_versions(list.tags.unwrap_or_default()))
    }
}

fn full_repo(config: &RegistryConfig, repo: &str) -> String {
    let project = config.project.trim().trim_matches('/');
    if project.is_empty() {
        repo.to_string()
    } else {
        format!("{project}/{repo}")
    }
}

fn refused(config: &RegistryConfig) -> String {
    if config.auth == "basic" {
        "the registry refused this user and password (or token)".into()
    } else {
        "the registry wants a login: choose \"user and password\"".into()
    }
}

fn unreachable(config: &RegistryConfig, e: &reqwest::Error) -> anyhow::Error {
    let why = if e.is_timeout() {
        "it did not answer in time"
    } else if e.is_connect() {
        "cannot connect"
    } else {
        "the request failed"
    };
    anyhow!(
        "{}: {why}. Check the address and that this hub can reach it ({})",
        config.host(),
        root_cause(e)
    )
}

fn root_cause(e: &dyn std::error::Error) -> String {
    let mut cur = e;
    while let Some(next) = cur.source() {
        cur = next;
    }
    cur.to_string()
}

/// Versions first, newest to oldest; `latest`, `dev` and the like after them.
pub fn sort_versions(mut tags: Vec<String>) -> Vec<String> {
    tags.sort_by(|a, b| {
        let (va, vb) = (crate::version::parse(a), crate::version::parse(b));
        match (va, vb) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.cmp(b),
        }
    });
    tags
}

#[cfg(test)]
#[path = "../../tests/unit/registry.rs"]
mod tests;
