use super::*;

fn rules(text: &str) -> Vec<Rule> {
    parse(text).unwrap()
}

fn can(text: &str, cap: Cap, path: &str) -> bool {
    let r = rules(text);
    decide(r.iter().map(|x| ("p", x)), cap, path).allowed()
}

#[test]
fn the_two_starting_policies_parse() {
    assert_eq!(rules(ADMIN)[0].caps.len(), 5);
    assert_eq!(
        rules(READ_ONLY)[0].caps,
        BTreeSet::from([Cap::Read, Cap::List])
    );
}

#[test]
fn a_policy_is_hcl_with_path_blocks_and_nothing_else() {
    let r = rules(
        r#"
        # sources are readable
        path "sources/*" { capabilities = ["read", "list"] }
        path "/alerts/+/ack/" { capabilities = ["create"] }
    "#,
    );
    assert_eq!(r.len(), 2);
    assert_eq!(
        r[1].path, "alerts/+/ack",
        "slashes at the ends do not count"
    );
    assert!(rules("").is_empty());
    for (bad, why) in [
        ("path \"a\" {", "not valid HCL"),
        ("x = 1", "unexpected `x`"),
        ("rule \"a\" { }", "unknown block `rule`"),
        ("path { capabilities = [\"read\"] }", "exactly one label"),
        (
            "path \"a\" \"b\" { capabilities = [\"read\"] }",
            "exactly one label",
        ),
        ("path \"a\" { }", "no capabilities"),
        ("path \"a\" { capabilities = \"read\" }", "is a list"),
        ("path \"a\" { capabilities = [1] }", "is a string"),
        (
            "path \"a\" { capabilities = [\"write\"] }",
            "unknown capability \"write\"",
        ),
        (
            "path \"a\" {\n capabilities = [\"read\"]\n other = 1\n}",
            "unknown setting `other`",
        ),
        (
            "path \"a*b\" { capabilities = [\"read\"] }",
            "`*` can only be the last",
        ),
        ("path \"\" { capabilities = [\"read\"] }", "cannot be empty"),
    ] {
        let e = parse(bad).unwrap_err().to_string();
        assert!(e.contains(why), "{bad:?}: {e}");
    }
}

#[test]
fn a_path_is_exact_or_a_prefix_and_plus_is_one_segment() {
    for (rule, path, yes) in [
        ("*", "anything/at/all", true),
        ("*", "", true),
        ("sources", "sources", true),
        ("sources", "sources/s1", false),
        ("sources/*", "sources/s1", true),
        ("sources/*", "sources/s1/upgrade", true),
        ("sources/*", "sources", true), // the list of what is below is part of it
        ("sources/*", "sourcesx", false),
        ("sources/prod-*", "sources/prod-eu", true),
        ("sources/prod-*", "sources/prod-eu/upgrade", true),
        ("sources/prod-*", "sources/dev-eu", false),
        ("sources/prod-*", "sources", false),
        ("alerts/+/ack", "alerts/7/ack", true),
        ("alerts/+/ack", "alerts/7/8/ack", false),
        ("alerts/+/ack", "alerts/7", false),
        ("dc/+/sources/*", "dc/eu/sources/s1", true),
        ("dc/+/sources/*", "dc/eu/alerts", false),
        ("users/3", "/users/3/", true),
    ] {
        assert_eq!(covers(rule, path), yes, "{rule} vs {path}");
    }
}

#[test]
fn the_method_says_what_is_asked_for() {
    for (m, c) in [
        ("GET", Cap::Read),
        ("POST", Cap::Create),
        ("PUT", Cap::Update),
        ("PATCH", Cap::Update),
        ("DELETE", Cap::Delete),
    ] {
        assert_eq!(Cap::of_method(m), Some(c));
    }
    assert_eq!(Cap::of_method("TRACE"), None);
}

#[test]
fn what_no_rule_covers_is_refused_and_deny_beats_everything() {
    let p = r#"
        path "sources/*" { capabilities = ["read", "list", "update"] }
        path "sources/prod-eu" { capabilities = ["deny"] }
        path "alerts" { capabilities = ["read"] }
    "#;
    assert!(can(p, Cap::Read, "sources/s1"));
    assert!(can(p, Cap::Update, "sources/s1"));
    assert!(!can(p, Cap::Delete, "sources/s1"), "no rule gives delete");
    assert!(
        !can(p, Cap::Read, "sources/prod-eu"),
        "deny wins over the rule above it"
    );
    assert!(!can(p, Cap::Read, "settings"), "nothing covers it");
    assert!(can(p, Cap::Read, "alerts"));
    assert!(!can(p, Cap::Create, "alerts"));
    // `list` lets a GET through, but not a write
    assert!(can(
        "path \"a\" { capabilities = [\"list\"] }",
        Cap::Read,
        "a"
    ));
    assert!(!can(
        "path \"a\" { capabilities = [\"list\"] }",
        Cap::Update,
        "a"
    ));
}

#[test]
fn several_policies_add_up_and_the_decision_says_which_ones() {
    let a = rules("path \"sources/*\" { capabilities = [\"read\"] }");
    let b = rules("path \"sources/*\" { capabilities = [\"update\"] }");
    let c = rules("path \"sources/s9\" { capabilities = [\"deny\"] }");
    let all = |extra: &[(&'static str, &Vec<Rule>)]| -> Vec<(&'static str, Rule)> {
        let mut v: Vec<(&str, Rule)> = vec![("reader", a[0].clone()), ("editor", b[0].clone())];
        for (n, r) in extra {
            v.push((n, r[0].clone()));
        }
        v
    };
    let rs = all(&[]);
    assert_eq!(
        decide(rs.iter().map(|(n, r)| (*n, r)), Cap::Update, "sources/s1"),
        Decision::Allowed {
            by: vec!["editor".into()]
        }
    );
    let rs = all(&[("freeze", &c)]);
    assert_eq!(
        decide(rs.iter().map(|(n, r)| (*n, r)), Cap::Read, "sources/s9"),
        Decision::Denied {
            by: vec!["freeze".into()]
        }
    );
    assert_eq!(
        decide(rs.iter().map(|(n, r)| (*n, r)), Cap::Delete, "sources/s1"),
        Decision::NoRule
    );
}
