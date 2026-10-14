use std::sync::Arc;

use super::*;
use crate::database::Repositories;

fn fed() -> FederationImp {
    FederationImp::new(Repositories::sqlite_in_memory().unwrap().federation)
}

const NOW: i64 = 1_000_000;

#[test]
fn a_code_pairs_once_and_gives_the_token_that_then_opens_the_hub() {
    let f = fed();
    let (code, expires) = f.enable_member("http://hub.dc2:8765/", NOW).unwrap();
    assert_eq!(expires, NOW + CODE_TTL_MS);
    assert_eq!(code.len(), 14);
    assert!(!f.is_locked(), "waiting is not yet managed");

    let token = f.pair(&code, "dc1", "dc2", NOW + 1).unwrap();
    assert!(f.is_locked() && f.is_member());
    assert!(f.verify_link(&token));
    assert!(!f.verify_link("not the token"));
    assert!(
        !f.verify_link(""),
        "an empty token never matches an empty hash"
    );
    assert!(
        matches!(f.pair(&code, "dc1", "dc2", NOW + 2), Err(FedError::Locked)),
        "the code is used up: nobody else can take the hub"
    );
    let view = f.member_view(NOW + 3).unwrap();
    assert_eq!(
        view.managed_by.as_deref(),
        Some("dc1"),
        "the master's name, not the name it gave this hub"
    );
    assert_eq!(view.datacenter.as_deref(), Some("dc2"));
    assert_eq!(
        view.agent_address, "http://hub.dc2:8765",
        "the trailing slash is dropped"
    );
}

#[test]
fn a_code_is_forgiving_about_case_and_spaces_but_not_about_time() {
    let f = fed();
    let (code, _) = f.enable_member("http://h:1", NOW).unwrap();
    let sloppy = format!(" {} ", code.to_lowercase().replace('-', " "));
    assert!(matches!(
        f.pair(&sloppy, "dc1", "dc2", NOW + CODE_TTL_MS),
        Err(FedError::Expired)
    ));
    assert!(f.pair(&sloppy, "dc1", "dc2", NOW + CODE_TTL_MS - 1).is_ok());
}

#[test]
fn a_code_that_was_guessed_wrong_too_often_is_dead_until_a_new_one_is_asked_for() {
    let f = fed();
    let (code, _) = f.enable_member("http://h:1", NOW).unwrap();
    for _ in 0..10 {
        assert!(matches!(
            f.pair("AAAA-BBBB-CCCC", "dc1", "dc2", NOW),
            Err(FedError::BadCode)
        ));
    }
    assert!(
        matches!(f.pair(&code, "dc1", "dc2", NOW), Err(FedError::BadCode)),
        "even the right code no longer works"
    );
    let (fresh, _) = f.new_code(NOW).unwrap();
    assert_ne!(fresh, code);
    assert!(f.pair(&fresh, "dc1", "dc2", NOW).is_ok());
}

#[test]
fn a_new_code_replaces_the_old_one() {
    let f = fed();
    let (old, _) = f.enable_member("http://h:1", NOW).unwrap();
    let (new, _) = f.new_code(NOW).unwrap();
    assert!(matches!(
        f.pair(&old, "dc1", "dc2", NOW),
        Err(FedError::BadCode)
    ));
    assert!(f.pair(&new, "dc1", "dc2", NOW).is_ok());
}

#[test]
fn a_hub_that_is_not_waiting_cannot_be_paired() {
    let f = fed();
    assert!(matches!(
        f.pair("AAAA", "dc1", "dc2", NOW),
        Err(FedError::NotWaiting)
    ));
    assert!(matches!(f.new_code(NOW), Err(FedError::NotWaiting)));
    assert!(!f.is_member());
    assert!(f.member_view(NOW).is_none());
}

#[test]
fn a_managed_hub_can_only_be_let_go_by_release() {
    let f = fed();
    let (code, _) = f.enable_member("http://h:1", NOW).unwrap();
    f.pair(&code, "dc1", "dc2", NOW).unwrap();
    assert!(matches!(f.disable_member(), Err(FedError::Locked)));
    assert!(matches!(f.new_code(NOW), Err(FedError::Locked)));
    assert!(matches!(
        f.enable_member("http://h:1", NOW),
        Err(FedError::Locked)
    ));
    f.release().unwrap();
    assert!(!f.is_member() && !f.is_locked());
}

#[test]
fn a_hub_that_waits_can_give_up() {
    let f = fed();
    f.enable_member("http://h:1", NOW).unwrap();
    f.disable_member().unwrap();
    assert!(!f.is_member());
}

#[test]
fn the_agent_address_must_be_a_url() {
    let f = fed();
    f.enable_member("http://h:1", NOW).unwrap();
    assert!(matches!(
        f.set_agent_address("h:1"),
        Err(FedError::Invalid(_))
    ));
    f.set_agent_address("https://hub.dc2.example/").unwrap();
    assert_eq!(
        f.member_view(NOW).unwrap().agent_address,
        "https://hub.dc2.example"
    );
}

#[test]
fn the_link_secret_is_this_process_only() {
    let (a, b) = (fed(), fed());
    assert!(a.is_link_secret(&a.link_secret()));
    assert!(!a.is_link_secret(&b.link_secret()));
    assert!(!a.is_link_secret(""));
}

fn dc(id: &str, name: &str) -> Datacenter {
    Datacenter {
        id: id.into(),
        name: name.into(),
        address: "http://x:1".into(),
        token: "secret".into(),
    }
}

#[test]
fn datacenters_have_names_of_their_own_and_a_hub_with_them_cannot_be_a_member() {
    let f = fed();
    f.add_datacenter(dc("d1", "dc2")).unwrap();
    assert!(matches!(
        f.add_datacenter(dc("d2", "DC2")),
        Err(FedError::Invalid(_))
    ));
    assert!(
        matches!(f.add_datacenter(dc("d3", "dc1")), Err(FedError::Invalid(_))),
        "the hub's own name is taken too"
    );
    assert!(matches!(
        f.enable_member("http://h:1", NOW),
        Err(FedError::Invalid(_))
    ));
    let renamed = f
        .update_datacenter("d1", Some("east"), Some("http://y:2/"))
        .unwrap();
    assert_eq!(
        (renamed.name.as_str(), renamed.address.as_str()),
        ("east", "http://y:2")
    );
    assert!(f.remove_datacenter("d1").unwrap().is_some());
    assert!(f.remove_datacenter("d1").unwrap().is_none());
}

#[test]
fn a_member_controls_no_datacenters() {
    let f = fed();
    f.enable_member("http://h:1", NOW).unwrap();
    assert!(matches!(
        f.add_datacenter(dc("d1", "dc2")),
        Err(FedError::Invalid(_))
    ));
}

#[test]
fn what_is_stored_keeps_no_secret_in_the_clear() {
    let ctx = Repositories::sqlite_in_memory().unwrap();
    let dao = ctx.federation.clone();
    let f = FederationImp::new(dao);
    f.add_datacenter(dc("d1", "dc2")).unwrap();
    f.set_name("home").unwrap();
    let raw: String = ctx
        .ctx
        .conn()
        .query_row(
            "SELECT value FROM settings WHERE key='federation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!raw.contains("secret"), "{raw}");
    assert_eq!(f.datacenters()[0].token, "secret", "and it reads back");
    assert_eq!(f.name(), "home");
    let _: Arc<dyn IFederation> = Arc::new(f);
}
