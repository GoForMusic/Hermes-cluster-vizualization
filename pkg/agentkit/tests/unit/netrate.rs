use super::*;

const T0: i64 = 1_000_000;
const fn at(rx: u64, tx: u64, secs: i64) -> Sample {
    Sample {
        rx,
        tx,
        at_ms: T0 + secs * 1000,
    }
}
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_rate_needs_two_samples_and_counts_from_a_new_baseline_after_a_reset() {
    let mut t = Tracker::new();
    assert_eq!(
        t.update("a", at(1_000_000, 0, 0)),
        None,
        "the first sample has nothing to compare with"
    );
    // 1.25 MB received and 0.5 MB sent in 10 s = 1 Mb/s and 0.4 Mb/s
    let (rx, tx) = t.update("a", at(2_250_000, 500_000, 10)).unwrap();
    assert!(close(rx, 1.0) && close(tx, 0.4), "{rx} {tx}");
    // a counter that goes backwards means the pod restarted: no rate for that round, and the new value is the baseline
    assert_eq!(t.update("a", at(10, 10, 20)), None);
    assert_eq!(
        t.update("a", at(10, 10, 20)),
        None,
        "time that did not advance"
    );
    assert!(close(t.update("a", at(1_250_010, 10, 30)).unwrap().0, 1.0));
    assert_eq!(t.update("b", at(0, 0, 0)), None, "another key starts fresh");
}

#[test]
fn keys_that_are_gone_are_forgotten_and_the_others_keep_their_baseline() {
    let mut t = Tracker::new();
    t.update("gone", at(0, 0, 0));
    t.update("stay", at(0, 0, 0));
    t.keep(&HashSet::from(["stay".to_string()]));
    assert_eq!(t.update("gone", at(1, 0, 1)), None);
    assert!(t.update("stay", at(1_000_000, 0, 1)).is_some());
}
