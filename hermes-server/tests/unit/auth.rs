use std::sync::atomic::{AtomicI64, Ordering};

use super::*;
use crate::database::Repositories;
use crate::model::SessionPolicy;

const PASSWORD: &str = "correct horse battery";

fn rig() -> (AuthImp, Arc<AtomicI64>) {
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let c = clock.clone();
    let db = Repositories::sqlite_in_memory().unwrap();
    (
        AuthImp::with_clock(
            db.users.clone(),
            db.groups.clone(),
            db.sessions.clone(),
            db.settings.clone(),
            Arc::new(move || c.load(Ordering::SeqCst)),
        ),
        clock,
    )
}

#[test]
fn a_hash_made_by_the_go_hub_verifies() {
    // made by the Go hub's own `auth.HashPassword("hunter2hunter2")` (golang.org/x/crypto/argon2): an existing admin keeps working
    let go = "$argon2id$v=19$m=65536,t=3,p=2$WPxEKIhQUFEvFhr2+6k3NQ$aaDsRn/+Ls7EtJ8dgEBMuY5cEGo8so688K389Lhb+Xk";
    assert!(verify_password("hunter2hunter2", go));
    assert!(!verify_password("hunter2hunter3", go));
}

#[test]
fn a_hash_made_here_has_the_same_format_and_verifies() {
    let mine = hash_password("hunter2hunter2");
    assert!(
        mine.starts_with("$argon2id$v=19$m=65536,t=3,p=2$"),
        "{mine}"
    );
    assert_eq!(mine.split('$').count(), 6);
    assert!(verify_password("hunter2hunter2", &mine));
    assert!(!verify_password("hunter2hunter3", &mine));
}

#[test]
fn malformed_hashes_never_verify() {
    for bad in [
        "",
        "plain",
        "$argon2id$v=19$m=1,t=1$salt$hash",
        "$argon2i$v=19$m=8,t=1,p=1$c2FsdHNhbHQ$aGFzaA",
        "$argon2id$v=19$m=99999999,t=1,p=1$c2FsdHNhbHQ$aGFzaA",
        "$argon2id$v=19$m=8,t=1,p=1$!!$!!",
    ] {
        assert!(!verify_password("x", bad), "{bad}");
    }
}

#[test]
fn setup_creates_the_first_admin_once() {
    let (auth, _) = rig();
    assert!(auth.needs_setup().unwrap());
    let token = auth.setup("  Admin ", PASSWORD, true).unwrap();
    assert_eq!(
        auth.authenticate(&token).unwrap().username,
        "admin",
        "the name is normalised"
    );
    assert!(auth.public_view());
    assert!(matches!(
        auth.setup("other", PASSWORD, false),
        Err(AuthError::SetupDone)
    ));
}

#[test]
fn setup_checks_the_username_and_the_password_length() {
    let (auth, _) = rig();
    for bad in ["ab", "has space", "UPPER!", &"x".repeat(33)] {
        assert!(
            matches!(
                auth.setup(bad, PASSWORD, false),
                Err(AuthError::BadUsername)
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        auth.setup("admin", "short", false),
        Err(AuthError::WeakPassword)
    ));
    assert!(auth.needs_setup().unwrap(), "nothing was created");
}

#[test]
fn login_needs_the_right_password_and_a_wrong_one_says_nothing_about_the_username() {
    let (auth, _) = rig();
    auth.setup("admin", PASSWORD, false).unwrap();
    assert!(auth.login("admin", PASSWORD, "1.2.3.4").is_ok());
    assert!(matches!(
        auth.login("admin", "wrong password!", "1.2.3.4"),
        Err(AuthError::BadCredentials)
    ));
    assert!(
        matches!(
            auth.login("nobody", PASSWORD, "1.2.3.4"),
            Err(AuthError::BadCredentials)
        ),
        "same error as a wrong password"
    );
}

#[test]
fn five_failures_block_the_address_for_ten_minutes_and_a_success_resets_the_count() {
    let (auth, clock) = rig();
    auth.setup("admin", PASSWORD, false).unwrap();
    for _ in 0..4 {
        let _ = auth.login("admin", "nope nope nope", "9.9.9.9");
    }
    auth.login("admin", PASSWORD, "9.9.9.9").unwrap(); // resets
    for _ in 0..5 {
        let _ = auth.login("admin", "nope nope nope", "9.9.9.9");
    }
    assert!(
        matches!(
            auth.login("admin", PASSWORD, "9.9.9.9"),
            Err(AuthError::RateLimited)
        ),
        "even the right password is refused now"
    );
    assert!(
        auth.login("admin", PASSWORD, "8.8.8.8").is_ok(),
        "another address is not affected"
    );
    clock.fetch_add(WINDOW_MS + 1, Ordering::SeqCst);
    assert!(auth.login("admin", PASSWORD, "9.9.9.9").is_ok());
}

#[test]
fn a_key_that_fails_once_and_is_never_retried_is_swept_away() {
    let (auth, clock) = rig();
    let _ = auth.login("ghost", "wrong", "1.1.1.1"); // one failure, nothing ever looks this key up again
    assert!(auth.limiter().fails.contains_key("1.1.1.1|ghost"));

    clock.fetch_add(WINDOW_MS + 1, Ordering::SeqCst);
    let _ = auth.login("ghost", "wrong", "2.2.2.2"); // an unrelated later failure piggybacks the sweep
    assert!(
        !auth.limiter().fails.contains_key("1.1.1.1|ghost"),
        "a key nobody ever retries must not sit in the map forever"
    );
}

#[test]
fn sessions_end_on_logout_and_when_they_expire() {
    let (auth, clock) = rig();
    auth.setup("admin", PASSWORD, false).unwrap();
    let a = auth.login("admin", PASSWORD, "ip").unwrap();
    let b = auth.login("admin", PASSWORD, "ip").unwrap();
    assert!(
        auth.authenticate(&a).is_some()
            && auth.authenticate("").is_none()
            && auth.authenticate("garbage").is_none()
    );
    auth.logout(&a);
    assert!(auth.authenticate(&a).is_none() && auth.authenticate(&b).is_some());
    clock.fetch_add(SessionPolicy::DEFAULT.max_ms() + 1, Ordering::SeqCst);
    assert!(auth.authenticate(&b).is_none());
}

#[test]
fn only_a_hash_of_the_session_token_is_stored() {
    let (auth, _) = rig();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    assert!(
        auth.sessions.session(&token).unwrap().is_none(),
        "the token itself is not a key"
    );
    assert!(
        auth.sessions
            .session(&hash_token(&token))
            .unwrap()
            .is_some()
    );
}

#[test]
fn changing_the_password_ends_every_other_session() {
    let (auth, _) = rig();
    let first = auth.setup("admin", PASSWORD, false).unwrap();
    let user = auth.authenticate(&first).unwrap();
    assert!(matches!(
        auth.change_password(user.id, "not the password", "another long one"),
        Err(AuthError::BadCredentials)
    ));
    assert!(matches!(
        auth.change_password(user.id, PASSWORD, "short"),
        Err(AuthError::WeakPassword)
    ));
    let fresh = auth
        .change_password(user.id, PASSWORD, "another long one")
        .unwrap();
    assert!(auth.authenticate(&first).is_none() && auth.authenticate(&fresh).is_some());
    assert!(matches!(
        auth.login("admin", PASSWORD, "ip"),
        Err(AuthError::BadCredentials)
    ));
    assert!(auth.login("admin", "another long one", "ip").is_ok());
}

#[test]
fn the_wallboard_is_private_until_the_admin_makes_it_public() {
    let (auth, _) = rig();
    assert!(!auth.public_view());
    auth.set_public_view(true).unwrap();
    assert!(auth.public_view());
    auth.set_public_view(false).unwrap();
    assert!(!auth.public_view());
}

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;

#[test]
fn a_login_lasts_fourteen_days_unless_the_admin_says_otherwise() {
    let (auth, _) = rig();
    assert_eq!(
        auth.session_policy(),
        SessionPolicy {
            max_hours: 336,
            idle_minutes: 0
        }
    );
    for bad in [
        SessionPolicy {
            max_hours: 0,
            idle_minutes: 0,
        },
        SessionPolicy {
            max_hours: 721,
            idle_minutes: 0,
        },
        SessionPolicy {
            max_hours: 8,
            idle_minutes: 4,
        },
        SessionPolicy {
            max_hours: 8,
            idle_minutes: 1441,
        },
    ] {
        assert!(
            matches!(
                auth.set_session_policy(bad),
                Err(AuthError::BadSessionPolicy)
            ),
            "{bad:?}"
        );
    }
    let ok = SessionPolicy {
        max_hours: 8,
        idle_minutes: 30,
    };
    auth.set_session_policy(ok).unwrap();
    assert_eq!(auth.session_policy(), ok);
}

#[test]
fn a_login_ends_after_the_longest_time_however_busy_the_person_is() {
    let (auth, clock) = rig();
    auth.set_session_policy(SessionPolicy {
        max_hours: 8,
        idle_minutes: 0,
    })
    .unwrap();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    for _ in 0..7 {
        clock.fetch_add(HOUR, Ordering::SeqCst);
        assert!(
            auth.authenticate(&token).is_some(),
            "busy, and still inside the 8 hours"
        );
    }
    clock.fetch_add(HOUR + 1, Ordering::SeqCst);
    assert!(auth.authenticate(&token).is_none());
    assert!(
        auth.sessions
            .session(&hash_token(&token))
            .unwrap()
            .is_none(),
        "and it is gone"
    );
}

#[test]
fn a_login_ends_after_a_while_without_a_request_and_every_request_starts_the_wait_again() {
    let (auth, clock) = rig();
    auth.set_session_policy(SessionPolicy {
        max_hours: 24,
        idle_minutes: 30,
    })
    .unwrap();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    for _ in 0..5 {
        clock.fetch_add(20 * MINUTE, Ordering::SeqCst);
        assert!(
            auth.authenticate(&token).is_some(),
            "20 minutes since the last request"
        );
    }
    clock.fetch_add(31 * MINUTE, Ordering::SeqCst);
    assert!(auth.authenticate(&token).is_none(), "31 minutes of nothing");
}

#[test]
fn asking_whether_a_login_is_alive_does_not_keep_it_alive() {
    let (auth, clock) = rig();
    auth.set_session_policy(SessionPolicy {
        max_hours: 24,
        idle_minutes: 30,
    })
    .unwrap();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    for _ in 0..4 {
        clock.fetch_add(10 * MINUTE, Ordering::SeqCst);
        // what a page with an open stream asks: it is open, but nobody is using it
        if clock.load(Ordering::SeqCst) < 1_000_000 + 30 * MINUTE {
            assert!(auth.is_alive(&token));
        }
    }
    assert!(!auth.is_alive(&token), "40 minutes without a real request");
}

#[test]
fn a_new_limit_counts_for_the_logins_that_already_exist() {
    let (auth, clock) = rig();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    clock.fetch_add(3 * HOUR, Ordering::SeqCst);
    assert!(
        auth.authenticate(&token).is_some(),
        "14 days: three hours is nothing"
    );
    auth.set_session_policy(SessionPolicy {
        max_hours: 2,
        idle_minutes: 0,
    })
    .unwrap();
    assert!(
        auth.authenticate(&token).is_none(),
        "now it is older than the longest login"
    );
}

#[test]
fn a_login_from_before_the_hub_kept_when_it_was_made_still_ends_at_its_own_end() {
    let (auth, clock) = rig();
    let token = auth.setup("admin", PASSWORD, false).unwrap();
    let hash = hash_token(&token);
    let row = auth.sessions.session(&hash).unwrap().unwrap();
    // the shape an older hub left: no creation time, no last use
    auth.sessions.delete_session(&hash).unwrap();
    auth.sessions
        .create_session(&hash, row.user_id, 0, row.expires)
        .unwrap();
    clock.fetch_add(HOUR, Ordering::SeqCst);
    assert!(auth.authenticate(&token).is_some());
    clock.store(row.expires + 1, Ordering::SeqCst);
    assert!(auth.authenticate(&token).is_none());
}
