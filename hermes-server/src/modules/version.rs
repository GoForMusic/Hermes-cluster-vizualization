//! Versions (semantic versioning). The release tag is the one source of truth: `server-1.2.3` and `agent-linux-1.2.3` reach the build as
//! `HERMES_VERSION` (the Dockerfile's `VERSION` argument). A build that did not get one is a development build and says so (`0.1.0-dev`), so
//! that it can never be mistaken for a published one.

use semver::Version;

/// The version of this hub.
pub const VERSION: &str = match option_env!("HERMES_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => concat!(env!("CARGO_PKG_VERSION"), "-dev"),
};

/// A version as agents write it: `1.2.3`, or `v1.2.3`.
pub fn parse(version: &str) -> Option<Version> {
    Version::parse(version.strip_prefix('v').unwrap_or(version)).ok()
}

/// The version an image reference stands for, from its tag: `registry:5000/infraviz/agent:1.2.3` is 1.2.3. `latest`, a digest or no tag at
/// all stand for no particular version.
pub fn image_version(image: &str) -> Option<Version> {
    let name = image.rsplit('/').next()?;
    let (_, tag) = name.rsplit_once(':')?;
    parse(tag)
}

/// Is an agent that reports `version` older than `expected`? An agent whose version cannot be read is not flagged: nothing is known about it.
pub fn is_outdated(version: &str, expected: &Version) -> bool {
    parse(version).is_some_and(|v| v < *expected)
}

#[cfg(test)]
#[path = "../../tests/unit/version.rs"]
mod tests;
