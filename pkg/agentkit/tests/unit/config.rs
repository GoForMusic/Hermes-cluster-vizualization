use super::*;

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |key| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_string())
    }
}

const BASE: [(&str, &str); 4] = [
    ("HUB_URL", "https://hub.example.com/"),
    ("SOURCE_ID", "s1"),
    ("SOURCE_NAME", "prod"),
    ("TOKEN", "t"),
];

#[test]
fn reads_the_settings_and_trims_the_url() {
    let cfg = Config::from_lookup("node", "1.2.3", env(&BASE)).unwrap();
    assert_eq!(cfg.hub_url, "https://hub.example.com");
    assert_eq!(
        (cfg.collector.as_str(), cfg.version.as_str()),
        ("node", "1.2.3")
    );
    assert_eq!(cfg.host, "");
    assert!(!cfg.agent_id.is_empty(), "defaults to the host name");
}

#[test]
fn a_missing_variable_is_named_in_the_error() {
    let err = Config::from_lookup("node", "1", env(&BASE[..3])).unwrap_err();
    assert!(err.to_string().contains("TOKEN"), "{err}");
}

#[test]
fn an_empty_variable_counts_as_missing() {
    let pairs = [
        ("HUB_URL", ""),
        ("SOURCE_ID", "s"),
        ("SOURCE_NAME", "n"),
        ("TOKEN", "t"),
    ];
    assert!(Config::from_lookup("node", "1", env(&pairs)).is_err());
}
