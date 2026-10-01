use super::*;
use crate::database::Repositories;
use crate::model::Source;
use crate::services::StoreImp;

fn setup(sources: &[&str]) -> (GuardImp, Arc<dyn IStore>, Arc<dyn ISourceDAO>) {
    let store: Arc<dyn IStore> = Arc::new(StoreImp::new());
    let db = Repositories::sqlite_in_memory().unwrap().sources;
    for id in sources {
        db.insert_source(&Source {
            id: (*id).into(),
            name: format!("name-{id}"),
            kind: "Kubernetes (agent)".into(),
            state: "pending".into(),
            ..Default::default()
        })
        .unwrap();
    }
    (GuardImp::new(store.clone(), db.clone()), store, db)
}

fn state_of(db: &Arc<dyn ISourceDAO>, id: &str) -> (String, String) {
    let s = db
        .list_sources()
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    (s.state, s.info)
}

fn cluster(id: &str, uid: &str) -> Vec<Node> {
    let meta = if uid.is_empty() {
        json!({})
    } else {
        json!({"uid": uid})
    };
    vec![Node {
        id: id.into(),
        kind: "cluster".into(),
        meta: serde_json::from_value(meta).unwrap(),
        ..Default::default()
    }]
}

#[test]
fn the_first_source_keeps_the_cluster_and_the_second_is_a_duplicate() {
    let (g, store, db) = setup(&["s1", "s2"]);
    assert_eq!(g.set_topology("s1", cluster("s1", "uid-1"), vec![]), Ok(()));
    assert_eq!(
        g.set_topology("s2", cluster("s2", "uid-1"), vec![]),
        Err(Duplicate)
    );
    assert_eq!(
        store
            .nodes()
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["s1"]
    );
    assert_eq!(
        state_of(&db, "s2"),
        (
            "duplicate".into(),
            "this cluster is already added as “name-s1”".into()
        )
    );
    assert_eq!(
        g.set_topology("s1", cluster("s1", "uid-1"), vec![]),
        Ok(()),
        "the owner can keep updating"
    );
}

#[test]
fn a_source_added_earlier_takes_the_cluster_over_after_a_restart() {
    let (g, store, db) = setup(&["s1", "s2"]);
    g.set_topology("s2", cluster("s2", "uid-1"), vec![])
        .unwrap(); // s2 reconnected first
    assert_eq!(g.set_topology("s1", cluster("s1", "uid-1"), vec![]), Ok(()));
    assert_eq!(
        store
            .nodes()
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["s1"]
    );
    assert_eq!(state_of(&db, "s2").0, "duplicate");
}

#[test]
fn different_clusters_and_clusters_without_a_uid_never_clash() {
    let (g, store, _) = setup(&["s1", "s2", "s3"]);
    assert_eq!(g.set_topology("s1", cluster("s1", "a"), vec![]), Ok(()));
    assert_eq!(g.set_topology("s2", cluster("s2", "b"), vec![]), Ok(()));
    assert_eq!(g.set_topology("s3", cluster("s3", ""), vec![]), Ok(()));
    assert_eq!(store.nodes().len(), 3);
}

#[test]
fn removing_the_owner_frees_the_cluster() {
    let (g, store, _) = setup(&["s1", "s2"]);
    g.set_topology("s1", cluster("s1", "uid-1"), vec![])
        .unwrap();
    g.remove_source("s1");
    assert!(store.nodes().is_empty());
    assert_eq!(g.set_topology("s2", cluster("s2", "uid-1"), vec![]), Ok(()));
}
