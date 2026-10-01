use super::*;

#[test]
fn this_build_has_a_valid_semantic_version() {
    assert!(parse(VERSION).is_some(), "{VERSION}");
    assert!(
        VERSION.ends_with("-dev") || option_env!("HERMES_VERSION").is_some(),
        "a build without a release version says so"
    );
}

#[test]
fn a_version_reads_with_or_without_a_leading_v() {
    assert_eq!(parse("1.2.3"), Version::parse("1.2.3").ok());
    assert_eq!(parse("v1.2.3"), parse("1.2.3"));
    assert_eq!(parse("1.2"), None);
    assert_eq!(parse("latest"), None);
    assert_eq!(parse(""), None);
}

#[test]
fn an_image_tag_is_its_version() {
    let v = |s: &str| image_version(s).map(|v| v.to_string());
    assert_eq!(v("hermes-agent-linux:1.2.3").as_deref(), Some("1.2.3"));
    assert_eq!(
        v("registry.example.com:5000/acm/hermes-agent-linux:v2.0.1").as_deref(),
        Some("2.0.1"),
        "a port is not a tag"
    );
    assert_eq!(
        v("git.example.com/acm/agent:1.0.0-rc.1").as_deref(),
        Some("1.0.0-rc.1")
    );
    for none in [
        "hermes-agent-linux",
        "hermes-agent-linux:latest",
        "hermes-agent-linux:dev",
        "registry:5000/agent",
        "agent@sha256:abcd",
    ] {
        assert_eq!(image_version(none), None, "{none}");
    }
}

#[test]
fn only_a_strictly_older_agent_is_outdated() {
    let expected = Version::parse("1.2.0").unwrap();
    assert!(is_outdated("1.1.9", &expected));
    assert!(
        is_outdated("1.2.0-rc.1", &expected),
        "a pre-release comes before its release"
    );
    assert!(is_outdated("0.1.0-dev", &expected));
    assert!(!is_outdated("1.2.0", &expected));
    assert!(!is_outdated("1.3.0", &expected), "newer is not outdated");
    assert!(!is_outdated("weird", &expected), "unreadable: not flagged");
}
