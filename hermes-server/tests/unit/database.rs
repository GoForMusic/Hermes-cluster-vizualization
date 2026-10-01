use rusqlite::Connection;

use super::*;
use crate::model::{Alert, Severity, Source};

fn src(id: &str) -> Source {
    Source {
        id: id.into(),
        name: id.into(),
        kind: "Kubernetes (agent)".into(),
        state: "pending".into(),
        secret: "tok".into(),
        ..Default::default()
    }
}

#[test]
fn settings_are_upserted() {
    let db = Repositories::sqlite_in_memory().unwrap();
    assert_eq!(db.settings.get_setting("k"), None);
    db.settings.set_setting("k", "1").unwrap();
    db.settings.set_setting("k", "2").unwrap();
    assert_eq!(db.settings.get_setting("k").as_deref(), Some("2"));
}

#[test]
fn sources_keep_their_creation_order_and_their_secret() {
    let db = Repositories::sqlite_in_memory().unwrap();
    db.sources.insert_source(&src("b")).unwrap();
    db.sources.insert_source(&src("a")).unwrap();
    db.sources
        .set_source_state("a", "connected", "3 nodes")
        .unwrap();
    let list = db.sources.list_sources().unwrap();
    assert_eq!(
        list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(
        (
            list[1].state.as_str(),
            list[1].info.as_str(),
            list[1].secret.as_str()
        ),
        ("connected", "3 nodes", "tok")
    );
    db.sources.delete_source("b").unwrap();
    assert_eq!(db.sources.list_sources().unwrap().len(), 1);
}

#[test]
fn alerts_can_be_resolved_and_listed_newest_first() {
    let db = Repositories::sqlite_in_memory().unwrap();
    let mut a = Alert {
        key: "host:h".into(),
        sev: Severity::Crit,
        node_id: "h".into(),
        title: "down".into(),
        ts: 10,
        ..Default::default()
    };
    let mut b = Alert {
        key: "host:i".into(),
        ts: 20,
        ..a.clone()
    };
    db.alerts.insert_alert(&mut a).unwrap();
    db.alerts.insert_alert(&mut b).unwrap();
    assert_eq!(db.alerts.active_alerts().unwrap().len(), 2);
    a.resolved_ts = Some(30);
    a.ack = true;
    db.alerts.update_alert(&a).unwrap();
    assert_eq!(db.alerts.active_alerts().unwrap().len(), 1);
    let list = db.alerts.list_alerts(10).unwrap();
    assert_eq!(list.iter().map(|x| x.id).collect::<Vec<_>>(), [b.id, a.id]);
    assert_eq!(db.alerts.get_alert(a.id).unwrap().unwrap(), a);
    assert_eq!(db.alerts.get_alert(999).unwrap(), None);
}

#[test]
fn users_and_sessions() {
    let db = Repositories::sqlite_in_memory().unwrap();
    assert_eq!(db.users.count_users().unwrap(), 0);
    let u = db.users.create_user("admin", "hash", "admin", 1).unwrap();
    assert!(
        db.users.create_user("admin", "other", "admin", 2).is_err(),
        "usernames are unique"
    );
    assert_eq!(db.users.user_by_name("admin").unwrap().unwrap().id, u.id);
    assert_eq!(
        db.users.user_by_id(u.id).unwrap().unwrap().password_hash,
        "hash"
    );
    assert!(db.users.user_by_name("nobody").unwrap().is_none());
    db.sessions.create_session("h1", u.id, 100).unwrap();
    db.sessions.create_session("h2", u.id, 300).unwrap();
    assert_eq!(db.sessions.session("h1").unwrap(), Some((u.id, 100)));
    db.sessions.delete_expired_sessions(200).unwrap();
    assert_eq!(
        (
            db.sessions.session("h1").unwrap(),
            db.sessions.session("h2").unwrap().is_some()
        ),
        (None, true)
    );
    db.sessions.delete_user_sessions(u.id).unwrap();
    assert!(db.sessions.session("h2").unwrap().is_none());
}

#[test]
fn an_existing_go_database_opens_unchanged() {
    // the Go hub's file has exactly this schema; opening it twice must not fail or change anything
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub.db");
    Repositories::sqlite(&path, Key::random())
        .unwrap()
        .settings
        .set_setting("k", "v")
        .unwrap();
    assert_eq!(
        Repositories::sqlite(&path, Key::random())
            .unwrap()
            .settings
            .get_setting("k")
            .as_deref(),
        Some("v")
    );
}

#[test]
fn a_legacy_plaintext_secret_is_encrypted_on_open() {
    // A row written before secrets were encrypted, or by the Go hub: `secret` is plain text in the column.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hub.db");
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO sources(id,name,type,endpoint,auth,state,info,secret,created) \
             VALUES('a','a','Kubernetes (agent)','','','pending','','plain-token',0)",
            [],
        )
        .unwrap();
    }
    let hex = "ab".repeat(32);
    let db = Repositories::sqlite(&path, Key::from_hex(&hex).unwrap()).unwrap();
    assert_eq!(db.sources.list_sources().unwrap()[0].secret, "plain-token");
    let raw: String = Connection::open(&path)
        .unwrap()
        .query_row("SELECT secret FROM sources WHERE id='a'", [], |r| r.get(0))
        .unwrap();
    assert!(raw.starts_with("enc1:"), "column still in clear: {raw}");
    drop(db);

    // reopening with the same key reads the now-encrypted secret back unchanged
    let db2 = Repositories::sqlite(&path, Key::from_hex(&hex).unwrap()).unwrap();
    assert_eq!(db2.sources.list_sources().unwrap()[0].secret, "plain-token");
}

#[test]
fn one_source_that_cannot_be_decrypted_does_not_take_the_others_down_with_it() {
    // A wrong/rotated HUB_SECRET_KEY, or a corrupted row, must not turn "list every source" into an error: rules,
    // dedupe and the admin page all call this and none of them should see every other cluster vanish over one row.
    let db = Repositories::sqlite_in_memory().unwrap();
    db.sources
        .insert_source(&Source {
            id: "good".into(),
            secret: "tok-good".into(),
            ..Default::default()
        })
        .unwrap();
    db.sources
        .insert_source(&Source {
            id: "bad".into(),
            secret: "tok-bad".into(),
            ..Default::default()
        })
        .unwrap();
    db.ctx
        .conn()
        .execute(
            "UPDATE sources SET secret='enc1:not-valid-base64!!!' WHERE id='bad'",
            [],
        )
        .unwrap();

    let list = db.sources.list_sources().unwrap();
    let get = |id: &str| list.iter().find(|s| s.id == id).unwrap().secret.clone();
    assert_eq!(get("good"), "tok-good", "unaffected by its neighbour");
    assert_eq!(
        get("bad"),
        "",
        "an undecryptable secret is treated as none, not as a reason to fail everything"
    );
}

#[test]
fn uptime_bars_show_the_worst_status_and_the_share_that_was_not_down() {
    let db = Repositories::sqlite_in_memory().unwrap();
    // window: 0..1000 ms in 10 buckets of 100 ms; the node is up, then down during 300..500, then up again
    db.beats.insert_beat("n", -50, "up").unwrap(); // before the window: what it was doing when the window opened
    db.beats.insert_beat("n", 300, "down").unwrap();
    db.beats.insert_beat("n", 500, "up").unwrap();
    let res = db.beats.uptime(1000, 1000, 10).unwrap();
    let n = &res["n"];
    assert_eq!(
        n.bars,
        [
            "up", "up", "up", "down", "down", "up", "up", "up", "up", "up"
        ]
    );
    assert!((n.pct - 80.0).abs() < 1e-9, "{}", n.pct);
    assert_eq!((n.current.as_str(), n.first), ("up", -50));
}

#[test]
fn the_registry_secret_is_stored_encrypted_and_read_back_whole() {
    let db = Repositories::sqlite_in_memory().unwrap();
    assert!(!db.registry.get_registry().is_set());
    let config = crate::model::RegistryConfig {
        url: "git.example.com".into(),
        project: "acm".into(),
        auth: "basic".into(),
        username: "robot".into(),
        secret: "s3cret".into(),
        ..Default::default()
    };
    db.registry.set_registry(&config).unwrap();
    assert_eq!(db.registry.get_registry(), config);
    let raw = db.settings.get_setting("registry").unwrap();
    assert!(!raw.contains("s3cret") && raw.contains("enc1:"), "{raw}");
}

#[test]
fn old_resolved_incidents_are_forgotten_and_open_ones_never() {
    let db = Repositories::sqlite_in_memory().unwrap();
    let alert = |ts: i64, resolved: Option<i64>| {
        let mut a = Alert {
            key: format!("k{ts}"),
            sev: Severity::Crit,
            node_id: "n".into(),
            title: "t".into(),
            ts,
            resolved_ts: resolved,
            ..Default::default()
        };
        db.alerts.insert_alert(&mut a).unwrap();
        a.resolved_ts = resolved;
        db.alerts.update_alert(&a).unwrap();
    };
    alert(100, Some(200)); // resolved long ago
    alert(100, None); // still open, and just as old
    alert(900, Some(1000)); // resolved recently
    assert_eq!(db.alerts.purge_resolved_before(500).unwrap(), 1);
    let left: Vec<i64> = db
        .alerts
        .list_alerts(10)
        .unwrap()
        .iter()
        .map(|a| a.ts)
        .collect();
    assert_eq!(left, [900, 100]);
}

#[test]
fn who_acknowledged_an_alert_and_when_survives_a_restart() {
    let db = Repositories::sqlite_in_memory().unwrap();
    let mut a = Alert {
        key: "k".into(),
        sev: Severity::Warn,
        node_id: "n".into(),
        title: "t".into(),
        ts: 5,
        ..Default::default()
    };
    db.alerts.insert_alert(&mut a).unwrap();
    a.ack = true;
    a.ack_by = "admin".into();
    a.ack_ts = Some(77);
    db.alerts.update_alert(&a).unwrap();
    let back = db.alerts.get_alert(a.id).unwrap().unwrap();
    assert_eq!(
        (back.ack, back.ack_by.as_str(), back.ack_ts),
        (true, "admin", Some(77))
    );
}
