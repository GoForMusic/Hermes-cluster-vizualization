use std::collections::HashMap;

use super::*;

fn of(pairs: &[(&str, &str)]) -> Option<String> {
    let labels: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    from_labels(|k| labels.get(k).map(String::as_str))
}

#[test]
fn a_node_that_says_nothing_has_no_location() {
    assert_eq!(of(&[]), None);
    assert_eq!(of(&[("location", "  ")]), None);
}

#[test]
fn the_explicit_label_wins_over_the_cloud_ones() {
    assert_eq!(
        of(&[
            ("hermes.io/location", "rack-2"),
            ("topology.kubernetes.io/zone", "a")
        ])
        .as_deref(),
        Some("rack-2")
    );
    assert_eq!(of(&[("location", "Cluj")]).as_deref(), Some("Cluj"));
}

#[test]
fn region_and_zone_are_joined_unless_they_are_the_same_word() {
    assert_eq!(
        of(&[
            ("topology.kubernetes.io/region", "eu-west-1"),
            ("topology.kubernetes.io/zone", "eu-west-1a")
        ])
        .as_deref(),
        Some("eu-west-1 / eu-west-1a")
    );
    assert_eq!(
        of(&[("topology.kubernetes.io/zone", "zone-b")]).as_deref(),
        Some("zone-b")
    );
    assert_eq!(
        of(&[
            ("topology.kubernetes.io/region", "x"),
            ("topology.kubernetes.io/zone", "x")
        ])
        .as_deref(),
        Some("x")
    );
}
