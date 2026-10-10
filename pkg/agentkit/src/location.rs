//! Where a machine is (a rack, a site, a region), read from the labels its cluster already has on the node, so nobody has to type it twice.

/// The label an admin sets to say it outright; Swarm labels have no prefix, so the bare `location` works there too.
const EXPLICIT: [&str; 3] = ["hermes.io/location", "hermes.location", "location"];
const REGION: &str = "topology.kubernetes.io/region";
const ZONE: &str = "topology.kubernetes.io/zone";

/// The location from a node's labels (`get` looks one up): the explicit label wins, else the cloud's region and zone ("eu-west-1 / eu-west-1a").
/// `None` when the node says nothing.
pub fn from_labels<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Option<String> {
    let value = |key: &str| get(key).map(str::trim).filter(|v| !v.is_empty());
    if let Some(v) = EXPLICIT.iter().find_map(|k| value(k)) {
        return Some(v.to_string());
    }
    match (value(REGION), value(ZONE)) {
        (Some(r), Some(z)) if r != z => Some(format!("{r} / {z}")),
        (Some(v), _) | (None, Some(v)) => Some(v.to_string()),
        (None, None) => None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/location.rs"]
mod tests;
