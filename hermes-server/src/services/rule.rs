//! The alert rules: which conditions raise an incident, and their thresholds. Pure configuration — no evaluation logic here.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Rule {
    pub(crate) enabled: bool,
    pub(crate) value: f64,
    pub(crate) crit: f64,
}

pub(crate) const HOST_DOWN: &str = "host-down";
pub(crate) const WORKLOAD_CRASH: &str = "workload-crash";
pub(crate) const VOLUME_USAGE: &str = "volume-usage";
pub(crate) const IAC_DRIFT: &str = "iac-drift";

pub(crate) fn defaults() -> HashMap<String, Rule> {
    [
        (
            HOST_DOWN,
            Rule {
                enabled: true,
                ..Rule::default()
            },
        ),
        (
            WORKLOAD_CRASH,
            Rule {
                enabled: true,
                ..Rule::default()
            },
        ),
        (
            VOLUME_USAGE,
            Rule {
                enabled: true,
                value: 85.0,
                crit: 95.0,
            },
        ),
        (
            IAC_DRIFT,
            Rule {
                enabled: true,
                ..Rule::default()
            },
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}
