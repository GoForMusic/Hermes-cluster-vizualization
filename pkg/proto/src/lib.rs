//! The gRPC contract between agents and the hub, generated from `proto/infraviz/v1/agent.proto`.

pub mod value;

pub mod v1 {
    tonic::include_proto!("infraviz.v1");
}

/// Version of the contract this build speaks (`Hello.protocol`).
pub const PROTOCOL: u32 = 1;

/// A version as the hub writes it and a registry tag can hold it: `1.2.3`, `1.2.3-rc.1`.
pub fn valid_version(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    version.len() <= 64
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}
