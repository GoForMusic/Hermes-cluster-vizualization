//! Supporting infrastructure that is not persistence (`database`), behavior/state (`services`) or an entry point
//! (`controller`): encryption, flow attribution, install manifests and version comparison. Each is a set of pure
//! functions with no state of its own — re-exported here under their own names so `crate::crypto::X` etc. elsewhere
//! is unaffected by living under this folder.

pub mod crypto;
pub mod flows;
pub mod manifest;
pub mod registry;
pub mod version;
