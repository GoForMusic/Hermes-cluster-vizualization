use hermes_agentkit::host::cpu_percent;

use super::*;

#[test]
fn cpu_percent_from_proc_stat() {
    let a =
        parse_cpu_times("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 1 1 1 1 1 1 1 1 1\n").unwrap();
    let b = parse_cpu_times("cpu  200 0 200 750 150 0 0 0 0 0\n").unwrap();
    // total went 1000 -> 1300 (+300); idle + iowait went 800 -> 900 (+100): 200 of 300 ticks busy = 66.67%
    assert!((cpu_percent(a, b) - 200.0 / 3.0).abs() < 0.01);
}

#[test]
fn garbage_is_refused() {
    for bad in ["", "nothing here", "cpu  1 2 x 4 5"] {
        assert!(parse_cpu_times(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn memory_in_use_is_what_is_not_available() {
    let (used, total) =
        parse_meminfo("MemTotal:  4194304 kB\nMemFree: 100 kB\nMemAvailable:  3145728 kB\n");
    assert!(
        (total - 4096.0).abs() < 0.01 && (used - 1024.0).abs() < 0.01,
        "{used} of {total}"
    );
}

#[test]
fn the_first_sample_has_no_cpu_and_this_machine_can_be_read() {
    let mut s = LinuxSampler::default();
    let first = s.sample().unwrap();
    assert_eq!(first.cpu, None);
    assert!(first.mem_total_mib > 0.0 && first.mem_used_mib > 0.0);
    assert!(s.sample().unwrap().cpu.is_some());
}
