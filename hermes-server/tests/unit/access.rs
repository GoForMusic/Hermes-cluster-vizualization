use super::*;
use std::sync::Arc;

use crate::database::Repositories;
use crate::services::AuthzImp;

struct Rig {
    access: AccessImp,
    db: Repositories,
}

fn rig() -> Rig {
    let db = Repositories::sqlite_in_memory().unwrap();
    let authz = Arc::new(AuthzImp::new(db.policies.clone()));
    let access = AccessImp::new(
        db.users.clone(),
        db.groups.clone(),
        db.policies.clone(),
        db.sessions.clone(),
        authz,
    );
    Rig { access, db }
}

/// A password for a test account, made for the run: nothing in the source is a password (and a scanner has nothing to take for one).
fn pw() -> String {
    format!("pw-{:016x}-long-enough", rand::random::<u64>())
}

impl Rig {
    /// The account the first-run setup makes.
    fn first(&self) -> UserView {
        self.access.create_user("root", &pw(), "", true).unwrap()
    }
    fn user(&self, name: &str) -> UserView {
        self.access.create_user(name, &pw(), "", false).unwrap()
    }
}

fn protected(r: Result<impl std::fmt::Debug, AccessError>) -> String {
    match r {
        Err(AccessError::Protected(why)) => why.to_string(),
        other => panic!("expected a protected account, got {other:?}"),
    }
}

#[test]
fn an_account_needs_a_good_name_a_long_password_and_a_real_email() {
    let r = rig();
    assert!(matches!(
        r.access.create_user("a", &pw(), "", false),
        Err(AccessError::BadName)
    ));
    assert!(matches!(
        r.access.create_user("alice", &pw()[..5], "", false),
        Err(AccessError::WeakPassword)
    ));
    for bad in ["nope", "a@b", "a b@c.d", "@c.d", "a@c."] {
        assert!(
            matches!(
                r.access.create_user("alice", &pw(), bad, false),
                Err(AccessError::BadEmail)
            ),
            "{bad}"
        );
    }
    let alice = r
        .access
        .create_user("  Alice ", &pw(), " alice@example.com ", false)
        .unwrap();
    assert_eq!(
        (alice.username.as_str(), alice.email.as_str()),
        ("alice", "alice@example.com")
    );
    assert!(matches!(
        r.access.create_user("ALICE", &pw(), "", false),
        Err(AccessError::Taken)
    ));
}

#[test]
fn the_password_is_stored_hashed_and_the_view_never_carries_it() {
    let r = rig();
    let alice = r.user("alice");
    let stored = r.db.users.user_by_id(alice.id).unwrap().unwrap();
    assert!(stored.password_hash.starts_with("$argon2id$"));
    assert!(!serde_json::to_string(&alice).unwrap().contains("argon2"));
}

#[test]
fn a_break_glass_account_is_never_disabled_or_deleted_and_one_always_remains() {
    let r = rig();
    let root = r.first();
    let bob = r.user("bob");
    let off = UserChanges {
        disabled: Some(true),
        ..UserChanges::default()
    };
    assert!(protected(r.access.update_user(bob.id, root.id, off)).contains("break-glass"));
    assert!(protected(r.access.delete_user(bob.id, root.id)).contains("break-glass"));
    // the flag is the only break-glass account's last: it cannot be taken off
    let unflag = || UserChanges {
        break_glass: Some(false),
        ..UserChanges::default()
    };
    assert!(protected(r.access.update_user(bob.id, root.id, unflag())).contains("at least one"));
    // with another one it can, and then the account is an ordinary one
    r.access
        .update_user(
            root.id,
            bob.id,
            UserChanges {
                break_glass: Some(true),
                ..UserChanges::default()
            },
        )
        .unwrap();
    assert!(
        !r.access
            .update_user(bob.id, root.id, unflag())
            .unwrap()
            .break_glass
    );
    r.access.delete_user(bob.id, root.id).unwrap();
    // and now bob's is the one that is left
    assert!(protected(r.access.update_user(root.id, bob.id, unflag())).contains("at least one"));
}

#[test]
fn nobody_disables_deletes_or_resets_their_own_account_from_the_admin_page() {
    let r = rig();
    r.first();
    let me = r.user("myself");
    let off = UserChanges {
        disabled: Some(true),
        ..UserChanges::default()
    };
    assert!(protected(r.access.update_user(me.id, me.id, off)).contains("your own"));
    assert!(protected(r.access.delete_user(me.id, me.id)).contains("your own"));
    let reset = UserChanges {
        password: Some(pw()),
        ..UserChanges::default()
    };
    assert!(protected(r.access.update_user(me.id, me.id, reset)).contains("Profile"));
}

#[test]
fn disabling_and_resetting_a_password_end_the_sessions_of_that_account() {
    let r = rig();
    let admin = r.first();
    let bob = r.user("bob");
    for (changes, what) in [
        (
            UserChanges {
                disabled: Some(true),
                ..UserChanges::default()
            },
            "disable",
        ),
        (
            UserChanges {
                password: Some(pw()),
                ..UserChanges::default()
            },
            "reset",
        ),
    ] {
        r.db.sessions
            .create_session("tok", bob.id, 0, i64::MAX)
            .unwrap();
        r.access.update_user(admin.id, bob.id, changes).unwrap();
        assert!(r.db.sessions.session("tok").unwrap().is_none(), "{what}");
    }
    let bob = r
        .access
        .users()
        .unwrap()
        .into_iter()
        .find(|u| u.id == bob.id)
        .unwrap();
    assert!(bob.disabled);
    // a weak password is refused before anything changes
    assert!(matches!(
        r.access.update_user(
            admin.id,
            bob.id,
            UserChanges {
                password: Some(pw()[..5].to_string()),
                ..UserChanges::default()
            }
        ),
        Err(AccessError::WeakPassword)
    ));
}

#[test]
fn deleting_an_account_takes_its_sessions_and_its_places_in_groups() {
    let r = rig();
    let admin = r.first();
    let bob = r.user("bob");
    let team = r.access.create_group("ops", "").unwrap();
    r.access.set_members(team.id, &[bob.id, admin.id]).unwrap();
    r.db.sessions
        .create_session("tok", bob.id, 0, i64::MAX)
        .unwrap();
    r.access.delete_user(admin.id, bob.id).unwrap();
    assert!(r.db.sessions.session("tok").unwrap().is_none());
    assert_eq!(r.access.groups().unwrap()[0].members, vec![admin.id]);
    assert!(matches!(
        r.access.delete_user(admin.id, bob.id),
        Err(AccessError::NotFound)
    ));
}

#[test]
fn groups_have_unique_names_and_exactly_the_members_that_were_set() {
    let r = rig();
    let a = r.user("alice");
    let b = r.user("bobby");
    let ops = r.access.create_group(" ops ", "the on-call").unwrap();
    assert_eq!(ops.name, "ops");
    assert!(matches!(
        r.access.create_group("ops", ""),
        Err(AccessError::Taken)
    ));
    assert!(matches!(
        r.access.create_group("  ", ""),
        Err(AccessError::BadGroupName)
    ));
    let dev = r.access.create_group("dev", "").unwrap();
    assert!(matches!(
        r.access.update_group(dev.id, Some("ops"), None),
        Err(AccessError::Taken)
    ));

    assert_eq!(
        r.access
            .set_members(ops.id, &[a.id, b.id, a.id])
            .unwrap()
            .members,
        vec![a.id, b.id]
    );
    assert_eq!(
        r.access.set_members(ops.id, &[b.id]).unwrap().members,
        vec![b.id]
    );
    assert!(matches!(
        r.access.set_members(ops.id, &[999]),
        Err(AccessError::NotFound)
    ));
    assert!(matches!(
        r.access.set_members(999, &[]),
        Err(AccessError::NotFound)
    ));
    // each user says which groups they are in
    let users = r.access.users().unwrap();
    assert_eq!(
        users.iter().find(|u| u.id == b.id).unwrap().groups,
        vec![ops.id]
    );
    assert!(
        users
            .iter()
            .find(|u| u.id == a.id)
            .unwrap()
            .groups
            .is_empty()
    );

    let renamed = r
        .access
        .update_group(ops.id, Some("on-call"), Some("pager"))
        .unwrap();
    assert_eq!(
        (renamed.name.as_str(), renamed.description.as_str()),
        ("on-call", "pager")
    );
    r.access.delete_group(ops.id).unwrap();
    assert!(r.db.groups.memberships().unwrap().is_empty());
    assert!(matches!(
        r.access.delete_group(ops.id),
        Err(AccessError::NotFound)
    ));
}

#[test]
fn an_older_database_gets_the_new_columns_and_its_first_admin_becomes_break_glass() {
    use rusqlite::Connection;
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL,
           role TEXT NOT NULL DEFAULT 'admin', created INTEGER NOT NULL);
         INSERT INTO users(username,password_hash,created) VALUES('first','h',1),('second','h',2);",
    )
    .unwrap();
    let ctx = std::sync::Arc::new(
        crate::database::DBContext_SQLite::from_connection(conn, crate::crypto::Key::random())
            .unwrap(),
    );
    let users = crate::database::UserDAOImp::new(ctx);
    use crate::database::IUserDAO;
    let all = users.list_users().unwrap();
    assert_eq!(
        all.iter()
            .map(|u| (u.username.as_str(), u.break_glass, u.disabled))
            .collect::<Vec<_>>(),
        vec![("first", true, false), ("second", false, false)]
    );
}

#[test]
fn an_update_finds_the_starting_policies_and_groups_with_every_old_account_an_owner_and_seeds_only_once()
 {
    use crate::database::{DBContext_SQLite, IDbContext, IGroupDAO, IPolicyDAO};
    use rusqlite::Connection;
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL,
           role TEXT NOT NULL DEFAULT 'admin', created INTEGER NOT NULL);
         INSERT INTO users(username,password_hash,created) VALUES('first','h',1),('second','h',2);",
    )
    .unwrap();
    let ctx =
        Arc::new(DBContext_SQLite::from_connection(conn, crate::crypto::Key::random()).unwrap());
    let policies = crate::database::PolicyDAOImp::new(ctx.clone());
    let groups = crate::database::GroupDAOImp::new(ctx.clone());

    let names: Vec<String> = policies
        .list_policies()
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["admin", "read-only"]);
    let owners = groups.group_by_name("owners").unwrap().unwrap();
    assert_eq!(
        groups.memberships().unwrap(),
        vec![(owners.id, 1), (owners.id, 2)],
        "every account that existed is an owner"
    );
    assert!(groups.group_by_name("everyone").unwrap().unwrap().system);
    // owners get the admin policy: an old account can still do everything
    let rules = policies.policies_of_user(1).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].0, "admin");

    // what the admin changes stays: starting again does not bring a deleted group back
    groups
        .delete_group(groups.group_by_name("viewers").unwrap().unwrap().id)
        .unwrap();
    crate::database::seed_access_again(ctx.conn()).unwrap();
    assert!(groups.group_by_name("viewers").unwrap().is_none());
}
