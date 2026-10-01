use super::*;

#[test]
fn the_busy_share_between_two_readings() {
    let prev = CpuTimes {
        total: 1000,
        idle: 800,
    };
    let cur = CpuTimes {
        total: 1400,
        idle: 1000,
    }; // 400 ticks passed, 200 of them idle: 50% busy
    assert!((cpu_percent(prev, cur) - 50.0).abs() < 0.01);
    assert_eq!(
        cpu_percent(cur, prev),
        0.0,
        "counters going backwards give 0"
    );
    assert_eq!(cpu_percent(prev, prev), 0.0, "no time passed gives 0");
}

#[test]
fn a_tracker_has_nothing_to_compare_its_first_reading_with() {
    let mut t = CpuTracker::default();
    let a = CpuTimes {
        total: 1000,
        idle: 800,
    };
    let b = CpuTimes {
        total: 1400,
        idle: 1000,
    };
    assert_eq!(
        t.update(a),
        None,
        "nothing to compare the first reading with"
    );
    assert!(
        (t.update(b).unwrap() - 50.0).abs() < 0.01,
        "the second compares against the first"
    );
    assert!(
        (t.update(b).unwrap() - 0.0).abs() < 0.01,
        "no time passed since the last one"
    );
}
