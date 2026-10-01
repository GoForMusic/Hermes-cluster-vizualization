use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const DEFAULT_LINUX_IMAGE: &str = "hermes-agent-linux";
pub const DEFAULT_WINDOWS_IMAGE: &str = "hermes-agent-windows";

/// How the hub logs in to the container registry the agent images come from. Stored in the settings table; the secret is
/// encrypted at rest and never sent to the browser (`RegistryView` says only that there is one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RegistryConfig {
    /// The registry's address: `git.example.com`, `harbor.example.com:8443`, `ghcr.io`, `registry-1.docker.io`; `http://` for a plain one.
    pub url: String,
    /// The project / user / organization the images live under: `acm`, `library`, `myorg`.
    pub project: String,
    /// `none` (public) or `basic` (a user and a password or access token; give it pull rights only).
    pub auth: String,
    pub username: String,
    pub secret: String,
    /// The repository names, under `project`.
    pub linux_image: String,
    pub windows_image: String,
}

impl RegistryConfig {
    pub fn is_set(&self) -> bool {
        !self.url.trim().is_empty()
    }

    /// The registry's host, without a scheme: what goes in front of an image name.
    pub fn host(&self) -> &str {
        let u = self.url.trim().trim_end_matches('/');
        u.strip_prefix("https://")
            .or_else(|| u.strip_prefix("http://"))
            .unwrap_or(u)
    }

    /// The address to call: `https` unless the person wrote `http://`.
    pub fn base(&self) -> String {
        if self.is_docker_hub() {
            return "https://registry-1.docker.io".into(); // Docker Hub's API lives on another name than the one images are written with
        }
        let u = self.url.trim().trim_end_matches('/');
        if u.starts_with("http://") || u.starts_with("https://") {
            u.to_string()
        } else {
            format!("https://{u}")
        }
    }

    fn is_docker_hub(&self) -> bool {
        matches!(
            self.host(),
            "docker.io" | "index.docker.io" | "registry-1.docker.io" | "hub.docker.com"
        )
    }

    /// The host as an image reference writes it: `docker.io` for Docker Hub.
    pub fn image_host(&self) -> &str {
        if self.is_docker_hub() {
            "docker.io"
        } else {
            self.host()
        }
    }

    pub fn linux_repo(&self) -> &str {
        non_empty(&self.linux_image, DEFAULT_LINUX_IMAGE)
    }

    pub fn windows_repo(&self) -> &str {
        non_empty(&self.windows_image, DEFAULT_WINDOWS_IMAGE)
    }

    /// `host/project/repo`, the image without its tag.
    pub fn image(&self, repo: &str) -> String {
        let project = self.project.trim().trim_matches('/');
        if project.is_empty() {
            format!("{}/{repo}", self.image_host())
        } else {
            format!("{}/{project}/{repo}", self.image_host())
        }
    }
}

fn non_empty<'a>(v: &'a str, default: &'a str) -> &'a str {
    if v.trim().is_empty() {
        default
    } else {
        v.trim()
    }
}

/// What the browser sees of the registry settings.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegistryView {
    pub url: String,
    pub project: String,
    pub auth: String,
    pub username: String,
    pub has_secret: bool,
    pub linux_image: String,
    pub windows_image: String,
}

impl From<&RegistryConfig> for RegistryView {
    fn from(c: &RegistryConfig) -> Self {
        Self {
            url: c.url.clone(),
            project: c.project.clone(),
            auth: if c.auth.is_empty() {
                "none".into()
            } else {
                c.auth.clone()
            },
            username: c.username.clone(),
            has_secret: !c.secret.is_empty(),
            linux_image: c.linux_repo().to_string(),
            windows_image: c.windows_repo().to_string(),
        }
    }
}

/// What the browser sends: the settings, with the secret only when it is being changed (absent = keep the stored one).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct RegistryInput {
    pub url: String,
    pub project: String,
    pub auth: String,
    pub username: String,
    #[ts(optional)]
    pub secret: Option<String>,
    pub linux_image: String,
    pub windows_image: String,
}

impl RegistryInput {
    /// The stored settings with this input applied.
    pub fn apply(self, stored: &RegistryConfig) -> RegistryConfig {
        let auth = if self.auth == "basic" {
            "basic"
        } else {
            "none"
        };
        RegistryConfig {
            url: self.url.trim().to_string(),
            project: self.project.trim().trim_matches('/').to_string(),
            auth: auth.into(),
            username: if auth == "basic" {
                self.username.trim().to_string()
            } else {
                String::new()
            },
            secret: match (auth, self.secret) {
                ("basic", Some(s)) if !s.is_empty() => s,
                ("basic", _) => stored.secret.clone(),
                _ => String::new(),
            },
            linux_image: self.linux_image.trim().to_string(),
            windows_image: self.windows_image.trim().to_string(),
        }
    }
}

/// The result of "Test connection", or of listing the versions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RegistryTest {
    pub ok: bool,
    pub message: String,
    /// The versions of the Linux agent image found in the registry, newest first.
    pub versions: Vec<String>,
}
