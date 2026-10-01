//! Generic helpers over the raw `kube::Client`: list every object of one kind, tolerate a list the service account has no permission
//! for, and a plain GET for the endpoints Kubernetes has no typed client for (a kubelet's stats summary, metrics-server).

use kube::api::ListParams;
use kube::{Api, Client};
use tracing::debug;

/// Every object of kind `K`, across all namespaces.
pub async fn list<K>(client: &Client) -> anyhow::Result<Vec<K>>
where
    K: kube::Resource<Scope = kube::core::NamespaceResourceScope>
        + Clone
        + serde::de::DeserializeOwned
        + std::fmt::Debug,
    K::DynamicType: Default,
{
    Ok(Api::<K>::all(client.clone())
        .list(&ListParams::default())
        .await?
        .items)
}

/// A list that may be refused: an agent installed before Services and Ingresses were read has no permission for them, and then the map
/// simply has no networks, as it has no numbers without metrics-server.
pub async fn list_optional<K>(client: &Client) -> Vec<K>
where
    K: kube::Resource<Scope = kube::core::NamespaceResourceScope>
        + Clone
        + serde::de::DeserializeOwned
        + std::fmt::Debug,
    K::DynamicType: Default,
{
    match list(client).await {
        Ok(items) => items,
        Err(e) => {
            debug!("{}: {e:#}", std::any::type_name::<K>());
            Vec::new()
        }
    }
}

/// A GET that may fail: metrics-server may not be installed and a kubelet may not answer.
pub async fn raw(client: &Client, path: &str) -> Option<String> {
    let request = http::Request::get(path).body(Vec::new()).ok()?;
    match client.request_text(request).await {
        Ok(text) => Some(text),
        Err(e) => {
            debug!("{path}: {e}");
            None
        }
    }
}
