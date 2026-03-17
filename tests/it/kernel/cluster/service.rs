use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::client::BendclawClient;
use bendclaw::client::ClusterClient;
use bendclaw::kernel::cluster::ClusterOptions;
use bendclaw::kernel::cluster::ClusterService;
use tokio_util::sync::CancellationToken;

use crate::common::fake_cluster::FakeClusterRegistry;
use crate::common::tracing;

fn make_service(registry_url: &str, auth_token: &str, node_id: &str) -> Arc<ClusterService> {
    let cluster_client = Arc::new(ClusterClient::new(
        registry_url,
        auth_token,
        node_id,
        format!("http://{node_id}.local"),
        "test-cluster",
    ));
    let bendclaw_client = Arc::new(BendclawClient::new(auth_token, Duration::from_secs(5)));
    Arc::new(ClusterService::with_options(
        cluster_client,
        bendclaw_client,
        ClusterOptions {
            heartbeat_interval: Duration::from_millis(100),
            dispatch_poll_interval: Duration::from_millis(25),
        },
    ))
}

#[tokio::test]
async fn register_and_discover_populates_peer_cache() -> Result<()> {
    tracing::init();
    let auth = "svc-test-token";
    let registry = FakeClusterRegistry::start(auth).await?;

    // Register a peer
    let peer = ClusterClient::new(
        registry.base_url(),
        auth,
        "node-peer",
        "http://peer.local",
        "test-cluster",
    );
    peer.register().await?;

    // Our service registers + discovers
    let svc = make_service(registry.base_url(), auth, "node-self");
    svc.register_and_discover().await?;

    let peers = svc.cached_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].node_id, "node-peer");
    assert_eq!(peers[0].endpoint, "http://peer.local");

    // meta() should return valid data
    let meta = peers[0].meta();
    assert_eq!(meta.status, "READY");
    assert!(meta.max_load > 0);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn register_and_discover_survives_discovery_failure() -> Result<()> {
    tracing::init();
    // Point at a non-existent registry — register_and_discover should not panic
    let svc = make_service("http://127.0.0.1:1", "bad-token", "node-self");
    // register_and_discover returns Err on register failure
    let result = svc.register_and_discover().await;
    assert!(result.is_err());
    // Cache should remain empty
    assert!(svc.cached_peers().is_empty());
    Ok(())
}

#[tokio::test]
async fn heartbeat_loop_refreshes_peers_and_cancels() -> Result<()> {
    tracing::init();
    let auth = "hb-test-token";
    let registry = FakeClusterRegistry::start(auth).await?;

    let svc = make_service(registry.base_url(), auth, "node-self");
    svc.register_and_discover().await?;
    assert!(svc.cached_peers().is_empty());

    // Register a peer after initial discovery
    let peer = ClusterClient::new(
        registry.base_url(),
        auth,
        "node-late",
        "http://late.local",
        "test-cluster",
    );
    peer.register().await?;

    // Start heartbeat loop — it should pick up the new peer
    let cancel = CancellationToken::new();
    let handle = svc.spawn_heartbeat(cancel.clone());

    // Wait for the heartbeat to refresh
    tokio::time::sleep(Duration::from_millis(300)).await;
    let peers = svc.cached_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].node_id, "node-late");

    // Cancel and verify clean shutdown
    cancel.cancel();
    handle.await?;

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn deregister_removes_from_registry() -> Result<()> {
    tracing::init();
    let auth = "dereg-test-token";
    let registry = FakeClusterRegistry::start(auth).await?;

    let svc = make_service(registry.base_url(), auth, "node-self");
    svc.register_and_discover().await?;
    assert_eq!(registry.snapshot().len(), 1);

    svc.deregister().await;
    assert_eq!(registry.snapshot().len(), 0);

    registry.shutdown().await;
    Ok(())
}

#[tokio::test]
async fn discover_excludes_self() -> Result<()> {
    tracing::init();
    let auth = "excl-test-token";
    let registry = FakeClusterRegistry::start(auth).await?;

    let svc = make_service(registry.base_url(), auth, "node-self");
    svc.register_and_discover().await?;

    // Only self is registered — peer list should be empty
    let peers = svc.cached_peers();
    assert!(peers.is_empty());

    // Add another node, refresh
    let peer = ClusterClient::new(
        registry.base_url(),
        auth,
        "node-other",
        "http://other.local",
        "test-cluster",
    );
    peer.register().await?;

    let peers = svc.refresh_peers().await?;
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].node_id, "node-other");

    // resolve_endpoint should work for the peer
    let ep = svc.resolve_endpoint("node-other")?;
    assert_eq!(ep, "http://other.local");

    // resolve_endpoint should fail for self
    assert!(svc.resolve_endpoint("node-self").is_err());

    registry.shutdown().await;
    Ok(())
}
