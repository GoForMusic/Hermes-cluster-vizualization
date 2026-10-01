//! What every agent shares: its settings from the environment, the gRPC uplink to the hub, and the loop that keeps a
//! collector running. An agent's `main()` only chooses the collector.

mod config;
pub mod differ;
pub mod host;
pub mod netrate;
mod queue;
mod runner;
mod sink;
pub mod testing;
mod upgrade;
mod uplink;

pub use config::Config;
pub use runner::{run, run_with};
pub use sink::{ISink, SharedSink};
pub use tonic::async_trait; // for implementing `IUpgrader` without a dependency of its own
pub use upgrade::{IUpgrader, SharedUpgrader, UpgradeOutcome, retag};
pub use uplink::{Timing, Uplink};
