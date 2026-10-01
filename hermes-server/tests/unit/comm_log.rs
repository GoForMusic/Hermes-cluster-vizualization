use serde_json::json;

use super::*;

fn entry(ts: i64, agent: &str, kind: &'static str, body: Value) -> NewEntry {
    NewEntry {
        ts,
        source: "s1".into(),
        source_name: "lab".into(),
        agent: agent.into(),
        dir: "in",
        kind,
        summary: "snapshot ×1".into(),
        bytes: 10,
        body,
    }
}

#[test]
fn it_records_nothing_until_it_is_turned_on_and_forgets_everything_when_turned_off() {
    let log = CommLogImp::new();
    log.record(entry(1, "a", "batch", json!({})));
    log.heartbeat();
    assert!(log.list(0, "", "", "", 10).entries.is_empty());
    assert_eq!(log.list(0, "", "", "", 10).heartbeats, 0);
    log.set_enabled(true);
    log.record(entry(1, "a", "batch", json!({"x": 1})));
    log.heartbeat();
    log.heartbeat();
    let v = log.list(0, "", "", "", 10);
    assert_eq!((v.entries.len(), v.heartbeats, v.enabled), (1, 2, true));
    log.set_enabled(false);
    assert!(
        log.list(0, "", "", "", 10).entries.is_empty(),
        "what was kept goes when it is turned off"
    );
}

#[test]
fn newest_first_and_only_what_is_newer_than_the_last_look() {
    let log = CommLogImp::new();
    log.set_enabled(true);
    for i in 0..5 {
        log.record(entry(i, "a", "batch", json!({"n": i})));
    }
    let v = log.list(0, "", "", "", 10);
    assert_eq!(
        v.entries.iter().map(|r| r.id).collect::<Vec<_>>(),
        [5, 4, 3, 2, 1]
    );
    assert_eq!(v.newest, 5);
    assert_eq!(
        log.list(3, "", "", "", 10)
            .entries
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        [5, 4]
    );
    assert_eq!(log.list(0, "", "", "", 2).entries.len(), 2, "a limit");
}

#[test]
fn it_keeps_the_last_five_hundred_and_the_last_fifteen_minutes() {
    let log = CommLogImp::new();
    log.set_enabled(true);
    for i in 0..(CAPACITY as i64 + 50) {
        log.record(entry(1_000_000 + i, "a", "batch", json!({})));
    }
    let v = log.list(0, "", "", "", 10_000);
    assert_eq!(v.entries.len(), CAPACITY);
    assert_eq!(v.entries.last().unwrap().id, 51, "the oldest went first");
    // an entry that is too old goes when a newer one comes
    let log = CommLogImp::new();
    log.set_enabled(true);
    log.record(entry(0, "a", "batch", json!({})));
    log.record(entry(
        i64::from(KEPT_MINUTES) * 60_000 + 1,
        "a",
        "batch",
        json!({}),
    ));
    assert_eq!(log.list(0, "", "", "", 10).entries.len(), 1);
}

#[test]
fn it_filters_by_source_by_kind_and_by_words_anywhere_including_the_content() {
    let log = CommLogImp::new();
    log.set_enabled(true);
    log.record(entry(
        1,
        "pod-a",
        "batch",
        json!({"nodes": [{"name": "checkout-7d9"}]}),
    ));
    log.record(NewEntry {
        source: "s2".into(),
        source_name: "prod".into(),
        ..entry(2, "pod-b", "resync", json!({}))
    });
    assert_eq!(log.list(0, "s2", "", "", 10).entries.len(), 1);
    assert_eq!(log.list(0, "", "resync", "", 10).entries.len(), 1);
    assert_eq!(
        log.list(0, "", "", "CHECKOUT", 10).entries.len(),
        1,
        "in the content, whatever the case"
    );
    assert_eq!(log.list(0, "", "", "pod-b", 10).entries[0].agent, "pod-b");
    assert!(
        log.list(0, "", "", "nothing like this", 10)
            .entries
            .is_empty()
    );
}

#[test]
fn a_content_is_kept_whole_or_cut_with_a_note_and_read_back_by_id() {
    let log = CommLogImp::new();
    log.set_enabled(true);
    log.record(entry(1, "a", "batch", json!({"small": true})));
    log.record(entry(2, "a", "batch", json!({"big": "é".repeat(MAX_BODY)})));
    assert_eq!(log.get(1).unwrap().body, json!({"small": true}));
    let big = log.get(2).unwrap().body;
    assert_eq!(big["truncated"], true);
    assert!(big["beginning"].as_str().unwrap().len() <= MAX_BODY);
    assert!(log.get(99).is_none());
    log.clear();
    assert!(log.get(1).is_none());
}

#[test]
fn a_batch_is_told_in_a_few_words_and_shown_whole() {
    use crate::model::Node;
    use crate::services::Event;
    let events = vec![
        Event::Snapshot {
            nodes: vec![Node {
                id: "s1:w:a".into(),
                name: "a".into(),
                ..Default::default()
            }],
            edges: vec![],
        },
        Event::Status {
            id: "s1:w:a".into(),
            own: "crit".into(),
            reason: "boom".into(),
        },
        Event::Status {
            id: "s1:w:b".into(),
            own: "ok".into(),
            reason: String::new(),
        },
        Event::Alive(vec!["s1:w:a".into()]),
    ];
    let (summary, body) = describe_batch(&events);
    assert_eq!(summary, "snapshot ×1 · status ×2 · alive ×1");
    assert_eq!(body["events"][1]["content"]["reason"], "boom");
    assert_eq!(body["events"][0]["content"]["nodes"][0]["name"], "a");
    // a big snapshot lists its first nodes and says how many were left out
    let many: Vec<Node> = (0..250)
        .map(|i| Node {
            id: format!("n{i}"),
            ..Default::default()
        })
        .collect();
    let (_, big) = describe_batch(&[Event::Snapshot {
        nodes: many,
        edges: vec![],
    }]);
    assert_eq!(
        (
            big["events"][0]["content"]["nodes"]
                .as_array()
                .unwrap()
                .len(),
            &big["events"][0]["content"]["moreNodes"]
        ),
        (200, &json!(50))
    );
}
