//! The JSON the browser sees. Field names and shapes are the ones of the Go hub, so the web app does not change.
//!
//! One type per file, named after the type it holds; this module just re-exports them, so `use crate::model::{Alert, Node, ...}`
//! elsewhere in the crate is unaffected by how they are organized in here.

mod add_source_request;
mod add_source_response;
mod agent_view;
mod alert;
mod auth_status;
mod comm_log;
mod edge;
mod flow_line;
mod hub_event;
mod hub_info;
mod node;
mod registry;
mod source;
mod source_view;
mod upgrade_view;
mod uptime;
mod user;

pub use add_source_request::AddSourceRequest;
pub use add_source_response::AddSourceResponse;
pub use agent_view::AgentView;
pub use alert::{Alert, Severity};
pub use auth_status::AuthStatus;
pub use comm_log::{CommLogEntry, CommLogRow, CommLogSwitch, CommLogView};
pub use edge::Edge;
pub use flow_line::FlowLine;
pub use hub_event::HubEvent;
pub use hub_info::HubInfo;
pub use node::{Meta, Node, own_name};
pub use registry::{
    DEFAULT_LINUX_IMAGE, DEFAULT_WINDOWS_IMAGE, RegistryConfig, RegistryInput, RegistryTest,
    RegistryView,
};
pub use source::{Source, TYPE_KUBERNETES_AGENT, TYPE_SWARM_AGENT, is_agent_type};
pub use source_view::SourceView;
pub use upgrade_view::{UpgradeRequest, UpgradeView};
pub use uptime::Uptime;
pub use user::User;

#[cfg(test)]
#[path = "../tests/unit/model.rs"]
mod tests;
