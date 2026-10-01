use serde_json::json;

use super::*;

fn stats(v: serde_json::Value) -> ContainerStats {
    serde_json::from_value(v).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_containers_cpu_is_a_share_of_the_machine_in_millicores_and_its_memory_excludes_the_page_cache()
{
    let s = stats(json!({
        "cpu_stats": {"cpu_usage": {"total_usage": 2_000_000}, "system_cpu_usage": 20_000_000, "online_cpus": 2},
        "precpu_stats": {"cpu_usage": {"total_usage": 1_000_000}, "system_cpu_usage": 10_000_000},
        "memory_stats": {"usage": 100 << 20, "stats": {"inactive_file": 40 << 20}},
    }));
    assert!(
        near(cpu_milli(&s), 200.0),
        "10% of the machine's 2 cores is 200 millicores"
    );
    assert!(near(mem_mib(&s), 60.0), "usage minus page cache");
}

#[test]
fn cgroup_v1_names_the_page_cache_differently_and_a_cache_bigger_than_the_usage_is_ignored() {
    assert!(near(
        mem_mib(&stats(
            json!({"memory_stats": {"usage": 100 << 20, "stats": {"cache": 10 << 20}}})
        )),
        90.0
    ));
    assert!(near(
        mem_mib(&stats(
            json!({"memory_stats": {"usage": 10 << 20, "stats": {"inactive_file": 50 << 20}}})
        )),
        10.0
    ));
}

#[test]
fn a_container_that_used_no_cpu_or_whose_counters_did_not_move_is_zero() {
    assert_eq!(cpu_milli(&ContainerStats::default()), 0.0);
    let same = stats(
        json!({"cpu_stats": {"cpu_usage": {"total_usage": 5}, "system_cpu_usage": 9}, "precpu_stats": {"cpu_usage": {"total_usage": 5}, "system_cpu_usage": 9}}),
    );
    assert_eq!(cpu_milli(&same), 0.0);
}

#[test]
fn a_windows_container_is_measured_against_the_wall_clock() {
    let s = stats(json!({
        "preread": "2026-01-01T12:00:00Z", "read": "2026-01-01T12:00:01Z", // 1 s = 10,000,000 intervals of 100 ns
        "precpu_stats": {"cpu_usage": {"total_usage": 1_000_000_000u64}},
        "cpu_stats": {"cpu_usage": {"total_usage": 1_005_000_000u64}}, // 0.5 s of CPU in that second: half a core
        "memory_stats": {"privateworkingset": 96 << 20},
    }));
    assert!(near(cpu_milli_windows(&s), 500.0));
    assert!(near(mem_mib_windows(&s), 96.0));
    assert_eq!(
        cpu_milli_windows(&stats(
            json!({"preread": "0001-01-01T00:00:00Z", "read": "0001-01-01T00:00:00Z"})
        )),
        0.0,
        "Docker's zero time"
    );
}
