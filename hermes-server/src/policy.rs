//! Policies, in the format Vault uses: HCL, one `path` block per rule.
//!
//! ```hcl
//! path "sources/*"       { capabilities = ["read", "list"] }
//! path "sources/prod-*"  { capabilities = ["deny"] }
//! ```
//!
//! A request is a *capability* on a *path*: the HTTP method gives the capability (GET `read`, POST `create`, PUT and PATCH `update`,
//! DELETE `delete`) and the URL without `/api/` (and without the base path) gives the path: `GET /api/sources/s1` is `read` on
//! `sources/s1`. A path in a rule is exact, or ends in `*` (anything below it), and a `+` stands for exactly one segment. Every rule that
//! matches counts: `deny` in any of them refuses the request, otherwise the request is allowed when some matching rule has its
//! capability. Nothing matching means no. `list` is today the same as `read` (a GET); it is kept apart so rules can already say it.

use std::collections::BTreeSet;
use std::fmt;

use hcl::Expression;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cap {
    Read,
    List,
    Create,
    Update,
    Delete,
    Deny,
}

impl Cap {
    pub const ALL: [Cap; 6] = [
        Cap::Read,
        Cap::List,
        Cap::Create,
        Cap::Update,
        Cap::Delete,
        Cap::Deny,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cap::Read => "read",
            Cap::List => "list",
            Cap::Create => "create",
            Cap::Update => "update",
            Cap::Delete => "delete",
            Cap::Deny => "deny",
        }
    }

    pub fn parse(name: &str) -> Option<Cap> {
        Cap::ALL.into_iter().find(|c| c.name() == name)
    }

    /// What an HTTP method needs.
    pub fn of_method(method: &str) -> Option<Cap> {
        match method {
            "GET" | "HEAD" | "OPTIONS" => Some(Cap::Read),
            "POST" => Some(Cap::Create),
            "PUT" | "PATCH" => Some(Cap::Update),
            "DELETE" => Some(Cap::Delete),
            _ => None,
        }
    }
}

/// One `path` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub path: String,
    pub caps: BTreeSet<Cap>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PolicyError(pub String);

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The rules of a policy, or what is wrong with it (with the line, when HCL knows it).
pub fn parse(text: &str) -> Result<Vec<Rule>, PolicyError> {
    let body = hcl::parse(text).map_err(|e| PolicyError(format!("not valid HCL: {e}")))?;
    if let Some(a) = body.attributes().next() {
        return Err(PolicyError(format!(
            "unexpected `{}`: a policy is only `path \"...\" {{ capabilities = [...] }}` blocks",
            a.key()
        )));
    }
    let mut rules = Vec::new();
    for block in body.blocks() {
        if block.identifier() != "path" {
            return Err(PolicyError(format!(
                "unknown block `{}`: only `path` blocks exist",
                block.identifier()
            )));
        }
        let [label] = block.labels() else {
            return Err(PolicyError(
                "a path block has exactly one label: path \"sources/*\" { ... }".into(),
            ));
        };
        let path = clean_path(label.as_str());
        if path.is_empty() && label.as_str() != "*" {
            return Err(PolicyError(
                "a path cannot be empty (use \"*\" for everything)".into(),
            ));
        }
        if let Some(i) = path.find('*').filter(|i| *i + 1 != path.len()) {
            return Err(PolicyError(format!(
                "path \"{path}\": `*` can only be the last character (found at {i})"
            )));
        }
        let mut caps = None;
        for attr in block.body().attributes() {
            if attr.key() != "capabilities" {
                return Err(PolicyError(format!(
                    "path \"{path}\": unknown setting `{}`, only `capabilities` exists",
                    attr.key()
                )));
            }
            let Expression::Array(items) = attr.expr() else {
                return Err(PolicyError(format!(
                    "path \"{path}\": capabilities is a list, like [\"read\", \"list\"]"
                )));
            };
            let mut set = BTreeSet::new();
            for item in items {
                let Expression::String(name) = item else {
                    return Err(PolicyError(format!(
                        "path \"{path}\": a capability is a string"
                    )));
                };
                let cap = Cap::parse(name).ok_or_else(|| {
                    PolicyError(format!(
                        "path \"{path}\": unknown capability \"{name}\" (use read, list, create, update, delete or deny)"
                    ))
                })?;
                set.insert(cap);
            }
            caps = Some(set);
        }
        let Some(caps) = caps else {
            return Err(PolicyError(format!("path \"{path}\" has no capabilities")));
        };
        rules.push(Rule { path, caps });
    }
    Ok(rules)
}

/// `/sources/s1/` -> `sources/s1`; the `*` of "everything" stays.
pub fn clean_path(path: &str) -> String {
    path.trim().trim_matches('/').to_string()
}

/// Does the rule's path cover the request's?
pub fn covers(rule: &str, path: &str) -> bool {
    let path = clean_path(path);
    if rule == "*" {
        return true;
    }
    let have: Vec<&str> = if path.is_empty() {
        vec![]
    } else {
        path.split('/').collect()
    };
    let same = |(w, h): (&&str, &&str)| *w == "+" || w == h;
    match rule.strip_suffix('*') {
        None => {
            let want: Vec<&str> = rule.split('/').collect();
            want.len() == have.len() && want.iter().zip(&have).all(same)
        }
        Some(prefix) => {
            // `sources/prod-*`: the directory `sources`, then a segment that starts with `prod-` (or `sources/*`: any, or none at all)
            let (dir, partial) = prefix
                .rfind('/')
                .map_or(("", prefix), |i| (&prefix[..i], &prefix[i + 1..]));
            let want: Vec<&str> = if dir.is_empty() {
                vec![]
            } else {
                dir.split('/').collect()
            };
            have.len() >= want.len()
                && want.iter().zip(&have).all(same)
                && have
                    .get(want.len())
                    .map_or(partial.is_empty(), |seg| seg.starts_with(partial))
        }
    }
}

/// What the rules say about one request.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed {
        by: Vec<String>,
    },
    Denied {
        by: Vec<String>,
    },
    /// No rule covers it.
    NoRule,
}

impl Decision {
    pub fn allowed(&self) -> bool {
        matches!(self, Decision::Allowed { .. })
    }
}

/// Decides over the rules of several policies: `(policy name, rule)`.
pub fn decide<'a>(
    rules: impl IntoIterator<Item = (&'a str, &'a Rule)>,
    cap: Cap,
    path: &str,
) -> Decision {
    let (mut allow, mut deny) = (Vec::new(), Vec::new());
    for (policy, rule) in rules {
        if !covers(&rule.path, path) {
            continue;
        }
        if rule.caps.contains(&Cap::Deny) {
            deny.push(policy.to_string());
        } else if rule.caps.contains(&cap) || (cap == Cap::Read && rule.caps.contains(&Cap::List)) {
            allow.push(policy.to_string());
        }
    }
    if !deny.is_empty() {
        Decision::Denied { by: dedup(deny) }
    } else if allow.is_empty() {
        Decision::NoRule
    } else {
        Decision::Allowed { by: dedup(allow) }
    }
}

fn dedup(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// The two policies every hub starts with.
pub const ADMIN: &str =
    "path \"*\" {\n  capabilities = [\"read\", \"list\", \"create\", \"update\", \"delete\"]\n}\n";
pub const READ_ONLY: &str = "path \"*\" {\n  capabilities = [\"read\", \"list\"]\n}\n";

#[cfg(test)]
#[path = "../tests/unit/policy.rs"]
mod tests;
