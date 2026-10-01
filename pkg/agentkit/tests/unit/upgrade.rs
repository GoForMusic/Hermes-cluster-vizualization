use super::*;

#[test]
fn only_the_tag_changes() {
    assert_eq!(
        retag("registry:5000/acm/agent:1.0.0", "1.0.1").unwrap(),
        "registry:5000/acm/agent:1.0.1"
    );
    assert_eq!(
        retag("git.example.com/acm/agent", "1.0.1").unwrap(),
        "git.example.com/acm/agent:1.0.1"
    );
    assert_eq!(
        retag("localhost:5000/agent:dev", "2.0.0-rc.1").unwrap(),
        "localhost:5000/agent:2.0.0-rc.1"
    );
    assert_eq!(
        retag("acm/agent-windows:1.0.0-ltsc2022", "1.0.1").unwrap(),
        "acm/agent-windows:1.0.1-ltsc2022"
    );
}

#[test]
fn a_digest_or_a_bad_version_is_refused() {
    assert!(retag("acm/agent@sha256:abcd", "1.0.1").is_err());
    for bad in [
        "",
        "latest",
        "1.0",
        "1.0.x",
        "../evil:1.0.1",
        "1.0.1/x",
        "1.0.1 ",
        "a.b.c",
    ] {
        assert!(retag("acm/agent:1.0.0", bad).is_err(), "{bad:?}");
    }
}
