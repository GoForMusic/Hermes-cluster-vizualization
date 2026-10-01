//! The Infra Viz hub.

pub mod app;
pub mod controller;
pub mod database;
pub mod model;
mod modules;
pub mod services;

// Re-exported at the crate root under their own names, so `crate::crypto::X`, `crate::flows::X`, etc. elsewhere in
// the crate are unaffected by living under `modules/` on disk.
pub use modules::{crypto, flows, manifest, registry, version};
