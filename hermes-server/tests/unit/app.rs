use super::*;

#[test]
fn the_listen_address_reads_like_the_go_hubs() {
    assert_eq!(parse_addr(":8765").unwrap().to_string(), "0.0.0.0:8765");
    assert_eq!(
        parse_addr("127.0.0.1:9").unwrap().to_string(),
        "127.0.0.1:9"
    );
    assert!(parse_addr("nonsense").is_err());
}

#[test]
fn incidents_are_kept_thirty_days_unless_the_admin_says_otherwise() {
    assert_eq!(incident_days(None), 30);
    assert_eq!(incident_days(Some("{}")), 30);
    assert_eq!(incident_days(Some("not json")), 30);
    assert_eq!(incident_days(Some(r#"{"incidentDays": 7}"#)), 7);
    assert_eq!(
        incident_days(Some(r#"{"incidentDays": 0}"#)),
        1,
        "never less than a day: 0 would forget everything at once"
    );
    assert_eq!(incident_days(Some(r#"{"incidentDays": 999999}"#)), 3650);
}

#[test]
fn the_base_path_is_a_plain_path_or_the_root() {
    for root in ["", "/", " ", "//", " / "] {
        assert_eq!(normalize_base_path(root).unwrap(), "", "{root:?}");
    }
    for (raw, want) in [
        ("/hermes", "/hermes"),
        ("hermes", "/hermes"),
        ("/hermes/", "/hermes"),
        ("//a//b/", "/a/b"),
        ("/infra-viz_2.0", "/infra-viz_2.0"),
    ] {
        assert_eq!(normalize_base_path(raw).unwrap(), want, "{raw:?}");
    }
    // it ends up in HTML, in a cookie and in a route: nothing that could break out of them
    for bad in [
        "/a b",
        "/a\"b",
        "/<script>",
        "/a;b",
        "/..",
        "/a/../b",
        "/a?x",
        "/%2e",
    ] {
        assert!(normalize_base_path(bad).is_err(), "{bad:?}");
    }
}
