use super::*;

#[test]
fn a_providers_id_is_made_from_its_name() {
    for (name, want) in [
        ("Microsoft Entra ID", "microsoft-entra-id"),
        ("  Google  ", "google"),
        ("Keycloak (lab) #2", "keycloak-lab-2"),
        ("???", "sso"),
    ] {
        assert_eq!(slug(name), want, "{name}");
    }
    assert!(slug(&"a".repeat(100)).len() <= 40);
}

#[test]
fn an_issuer_is_https_or_a_provider_on_this_machine() {
    for ok in [
        "https://login.microsoftonline.com/t-1/v2.0",
        "https://accounts.google.com",
        "http://localhost:8080/realms/x",
        "http://127.0.0.1:9",
    ] {
        assert!(issuer_ok(ok), "{ok}");
    }
    for bad in [
        "http://idp.example.com",
        "ftp://x",
        "login.example.com",
        "",
        "https://",
        "javascript:alert(1)",
    ] {
        assert!(!issuer_ok(bad), "{bad}");
    }
}

fn jwt(payload: &str) -> String {
    format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(payload))
}

#[test]
fn the_groups_claim_is_a_list_or_one_value_and_absent_is_not_empty() {
    assert_eq!(
        groups_of(&jwt(r#"{"groups":["a","b"]}"#), "groups"),
        Some(vec!["a".into(), "b".into()])
    );
    assert_eq!(
        groups_of(&jwt(r#"{"roles":"admin"}"#), "roles"),
        Some(vec!["admin".into()])
    );
    assert_eq!(groups_of(&jwt(r#"{"groups":[]}"#), "groups"), Some(vec![]));
    assert_eq!(
        groups_of(&jwt(r#"{"groups":["a",7,null]}"#), "groups"),
        Some(vec!["a".into()])
    );
    // no claim, a claim of another kind, or no token: nothing is said, which is not the same as "in no group"
    assert_eq!(groups_of(&jwt(r#"{"sub":"x"}"#), "groups"), None);
    assert_eq!(groups_of(&jwt(r#"{"groups":5}"#), "groups"), None);
    assert_eq!(groups_of("garbage", "groups"), None);
}
