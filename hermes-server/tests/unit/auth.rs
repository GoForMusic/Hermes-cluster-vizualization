use std::sync::atomic::{AtomicI64, Ordering};

use super::*;
use crate::database::Repositories;

const PASSWORD: &str = "correct horse battery";

fn rig() -> (AuthImp, Arc<AtomicI64>) {
    let clock = Arc::new(AtomicI64::new(1_000_000));
    let c = clock.clone();
    let db = Repositories::sqlite_in_memory().unwrap();
    (
        AuthImp::with_clock(
            db.users.clone(),
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
    clock.fetch_add(SESSION_TTL_MS + 1, Ordering::SeqCst);
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
