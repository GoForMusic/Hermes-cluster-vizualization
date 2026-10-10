//! The Docker Engine API, only the fields this collector uses. Docker writes its JSON in PascalCase, acronyms in capitals (`NodeID`, `NCPU`).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct EngineInfo {
    /// The engine's own id: what tells one Docker machine from another when it is not in a swarm.
    #[serde(rename = "ID")]
    pub id: String,
    /// The machine's host name.
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "OperatingSystem")]
    pub operating_system: String,
    #[serde(rename = "Architecture")]
    pub architecture: String,
    #[serde(rename = "ServerVersion")]
    pub server_version: String,
    #[serde(rename = "NCPU")]
    pub ncpu: i64,
    /// `linux` or `windows`
    #[serde(rename = "OSType")]
    pub os_type: String,
    #[serde(rename = "MemTotal")]
    pub mem_total: i64,
    #[serde(rename = "Swarm")]
    pub swarm: SwarmInfo,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SwarmInfo {
    #[serde(rename = "NodeID")]
    pub node_id: String,
    /// Only a manager can answer questions about the swarm.
    #[serde(rename = "ControlAvailable")]
    pub control_available: bool,
    #[serde(rename = "Cluster")]
    pub cluster: Option<ClusterInfo>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ClusterInfo {
    #[serde(rename = "ID")]
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SwarmNode {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Spec")]
    pub spec: NodeSpec,
    #[serde(rename = "Description")]
    pub description: NodeDescription,
    #[serde(rename = "Status")]
    pub status: NodeStatus,
    #[serde(rename = "ManagerStatus")]
    pub manager_status: Option<ManagerStatus>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NodeSpec {
    #[serde(rename = "Role")]
    pub role: String,
    #[serde(rename = "Availability")]
    pub availability: String,
    /// The labels an admin put on the node (`docker node update --label-add location=rack-2`).
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NodeDescription {
    #[serde(rename = "Hostname")]
    pub hostname: String,
    #[serde(rename = "Platform")]
    pub platform: Platform,
    #[serde(rename = "Resources")]
    pub resources: Resources,
    #[serde(rename = "Engine")]
    pub engine: EngineDescription,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Platform {
    #[serde(rename = "OS")]
    pub os: String,
    #[serde(rename = "Architecture")]
    pub architecture: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Resources {
    #[serde(rename = "NanoCPUs")]
    pub nano_cpus: i64,
    #[serde(rename = "MemoryBytes")]
    pub memory_bytes: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct EngineDescription {
    #[serde(rename = "EngineVersion")]
    pub engine_version: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NodeStatus {
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Addr")]
    pub addr: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ManagerStatus {
    #[serde(rename = "Reachability")]
    pub reachability: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SwarmService {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Spec")]
    pub spec: ServiceSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ServiceSpec {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(rename = "Mode")]
    pub mode: ServiceMode,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ServiceMode {
    /// Present (an empty object) for a global service: one task per node.
    #[serde(rename = "Global")]
    pub global: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SwarmTask {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "ServiceID")]
    pub service_id: String,
    #[serde(rename = "NodeID")]
    pub node_id: String,
    #[serde(rename = "Slot")]
    pub slot: i64,
    #[serde(rename = "DesiredState")]
    pub desired_state: String,
    #[serde(rename = "UpdatedAt")]
    pub updated_at: String,
    #[serde(rename = "Status")]
    pub status: TaskStatus,
    #[serde(rename = "Spec")]
    pub spec: TaskSpec,
    /// The networks the task is attached to (null while it has none).
    #[serde(rename = "NetworksAttachments")]
    pub networks: Option<Vec<NetworkAttachment>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NetworkAttachment {
    #[serde(rename = "Network")]
    pub network: AttachedNetwork,
    #[serde(rename = "Addresses")]
    pub addresses: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AttachedNetwork {
    #[serde(rename = "ID")]
    pub id: String,
}

/// A network as `/networks` lists it. On a manager it includes every swarm-scoped network, whichever node the members are on.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SwarmNetwork {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Scope")]
    pub scope: String,
    #[serde(rename = "Internal")]
    pub internal: bool,
    /// The routing mesh's own network: every service that publishes a port is on it, so it says nothing about who talks to whom.
    #[serde(rename = "Ingress")]
    pub ingress: bool,
    #[serde(rename = "IPAM")]
    pub ipam: Ipam,
    #[serde(rename = "Options")]
    pub options: Option<HashMap<String, String>>,
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Ipam {
    #[serde(rename = "Config")]
    pub config: Option<Vec<IpamConfig>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct IpamConfig {
    #[serde(rename = "Subnet")]
    pub subnet: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TaskStatus {
    #[serde(rename = "State")]
    pub state: String,
    #[serde(rename = "Timestamp")]
    pub timestamp: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Err")]
    pub err: String,
    #[serde(rename = "ContainerStatus")]
    pub container_status: ContainerStatus,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContainerStatus {
    #[serde(rename = "ContainerID")]
    pub container_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TaskSpec {
    #[serde(rename = "ContainerSpec")]
    pub container_spec: ContainerSpec,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContainerSpec {
    #[serde(rename = "Image")]
    pub image: String,
}

/// A container on this node, as `/containers/json` lists it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LocalContainer {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(rename = "Mounts")]
    pub mounts: Option<Vec<Mount>>,
    #[serde(rename = "NetworkSettings")]
    pub network_settings: Option<ContainerNetworks>,
}

/// The networks a container is attached to, by name (a swarm task's overlay networks, and `ingress` when it publishes a port).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContainerNetworks {
    #[serde(rename = "Networks")]
    pub networks: Option<HashMap<String, ContainerNetwork>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct ContainerNetwork {
    #[serde(rename = "NetworkID")]
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Mount {
    /// `volume`, `bind` or `tmpfs`
    #[serde(rename = "Type")]
    pub kind: String,
    #[serde(rename = "Name")]
    pub name: String,
}

/// `/system/df`: only the volumes are used.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SystemDf {
    #[serde(rename = "Volumes")]
    pub volumes: Option<Vec<VolumeInfo>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VolumeInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Driver")]
    pub driver: String,
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(rename = "UsageData")]
    pub usage: Option<VolumeUsage>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct VolumeUsage {
    /// Bytes the volume takes; -1 when the engine has not measured it.
    #[serde(rename = "Size")]
    pub size: i64,
}

/// One entry of `/containers/json?all=1`: any container on the machine, swarm or not.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContainerSummary {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Names")]
    pub names: Vec<String>,
    #[serde(rename = "Image")]
    pub image: String,
    /// `running`, `exited`, `restarting`, `paused`, `created` or `dead`
    #[serde(rename = "State")]
    pub state: String,
    /// The human line, like `Exited (1) 2 hours ago` or `Up 3 days (unhealthy)`.
    #[serde(rename = "Status")]
    pub status: String,
    /// Unix seconds.
    #[serde(rename = "Created")]
    pub created: i64,
    #[serde(rename = "Labels")]
    pub labels: Option<HashMap<String, String>>,
    /// The networks it is attached to, by name.
    #[serde(rename = "NetworkSettings")]
    pub network_settings: Option<ContainerNetworks>,
}
