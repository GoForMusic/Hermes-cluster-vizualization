//! The "Service" layer — everything that has behavior and state, as opposed to `database` (persistence) or `model` (data shape).
//! `IEngine`/`EngineImp` watches the live state once a second, opens and resolves alerts and records heartbeats; `IAuth`/`AuthImp`
//! is the admin login; `IIngestService`/`IngestServiceImp` accepts what agents report; `IStore`/`StoreImp` is the live topology
//! everything else reads and pushes to browsers; `IGuard`/`GuardImp` keeps the same cluster from being shown twice. `judge` and
//! `rule` are pure evaluation helpers `EngineImp` calls — no interface, since nothing is ever swapped in for a pure function.

mod auth_imp;
mod comm_log_imp;
mod engine_imp;
mod guard_imp;
mod ingest_service_imp;
mod judge;
mod rule;
mod store_imp;
mod upgrade_service_imp;

pub use auth_imp::{AuthError, AuthImp, IAuth, SESSION_TTL_MS};
pub use comm_log_imp::{CommLogImp, ICommLog, NewEntry, SharedCommLog, describe_batch};
pub use engine_imp::{EngineImp, IEngine};
pub use guard_imp::{Duplicate, GuardImp, IGuard};
pub use ingest_service_imp::{AgentInfo, Event, HelloInfo, IIngestService, IngestServiceImp};
pub use store_imp::{IStore, StoreImp};
pub use upgrade_service_imp::{
    IUpgradeService, SharedUpgrades, ToAgent, UpgradeError, UpgradeServiceImp,
};
// Only the test module (via `use super::*;`) needs these; gated so a normal build does not warn about them as unused.
#[cfg(test)]
pub(crate) use engine_imp::HOLD_DOWN_MS;
#[cfg(test)]
pub(crate) use judge::{Beat, fmt_size, judge};
#[cfg(test)]
pub(crate) use rule::{Rule, defaults};

#[cfg(test)]
#[path = "../tests/unit/services.rs"]
mod tests;
