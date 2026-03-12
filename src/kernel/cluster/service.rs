use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use super::DispatchTable;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::client::BendclawClient;
use crate::client::ClusterClient;
use crate::client::NodeInfo;

/// Unified cluster abstraction owning registration, peer cache, and node-to-node client.
/// Runtime holds a single `Arc<ClusterService>` instead of scattered fields.
pub struct ClusterService {
    cluster_client: Arc<ClusterClient>,
    bendclaw_client: Arc<BendclawClient>,
    /// Cached peer list, refreshed by heartbeat loop and cluster_nodes tool.
    peers: RwLock<Vec<NodeInfo>>,
}

impl ClusterService {
    pub fn new(cluster_client: Arc<ClusterClient>, bendclaw_client: Arc<BendclawClient>) -> Self {
        Self {
            cluster_client,
            bendclaw_client,
            peers: RwLock::new(Vec::new()),
        }
    }

    /// Return the last cached peer snapshot (never blocks on network).
    pub fn cached_peers(&self) -> Vec<NodeInfo> {
        self.peers.read().clone()
    }

    /// Refresh the peer cache from the registry.
    pub async fn refresh_peers(&self) -> Result<Vec<NodeInfo>> {
        let nodes = self.cluster_client.discover().await?;
        *self.peers.write() = nodes.clone();
        Ok(nodes)
    }
    /// Resolve a node_id to its endpoint from the cached peer list.
    /// Prevents SSRF by only allowing dispatch to known registered nodes.
    pub fn resolve_endpoint(&self, node_id: &str) -> Result<String> {
        let peers = self.peers.read();
        peers
            .iter()
            .find(|n| n.instance_id == node_id)
            .map(|n| n.endpoint.clone())
            .ok_or_else(|| {
                ErrorCode::cluster_dispatch(format!(
                    "unknown node_id '{node_id}' — not found in peer list. \
                     Call cluster_nodes to refresh."
                ))
            })
    }

    /// Create a DispatchTable backed by this service's BendclawClient.
    pub fn create_dispatch_table(self: &Arc<Self>) -> Arc<DispatchTable> {
        Arc::new(DispatchTable::new(self.bendclaw_client.clone()))
    }

    /// Register this node, do initial peer discovery, and return self.
    pub async fn register_and_discover(self: &Arc<Self>) -> Result<()> {
        self.cluster_client.register().await?;
        match self.refresh_peers().await {
            Ok(nodes) => {
                tracing::info!(peer_count = nodes.len(), "initial peer discovery done");
            }
            Err(e) => {
                tracing::warn!(error = %e, "initial peer discovery failed, starting with empty cache");
            }
        }
        Ok(())
    }

    /// Spawn the heartbeat + peer refresh loop. Returns the JoinHandle.
    pub fn spawn_heartbeat(
        self: &Arc<Self>,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let svc = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = svc.cluster_client.heartbeat().await {
                            tracing::warn!(error = %e, "cluster heartbeat failed");
                        }
                        if let Err(e) = svc.refresh_peers().await {
                            tracing::warn!(error = %e, "peer refresh failed");
                        }
                    }
                    _ = cancel.cancelled() => {
                        tracing::info!("cluster heartbeat stopped");
                        break;
                    }
                }
            }
        })
    }

    /// Deregister from the cluster registry.
    pub async fn deregister(&self) {
        if let Err(e) = self.cluster_client.deregister().await {
            tracing::warn!(error = %e, "cluster deregistration failed");
        }
    }
}
