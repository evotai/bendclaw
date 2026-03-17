use std::sync::Arc;

use async_trait::async_trait;

use crate::base::Result;
use crate::storage::pool::Pool;

/// Callback to release a lease from held state after async work completes.
/// Removes the resource from the in-memory held map and releases the DB lease.
pub type ReleaseFn = Arc<dyn Fn(&str) + Send + Sync>;

/// A discoverable resource entry with its current lease state from DB.
pub struct ResourceEntry {
    pub id: String,
    pub pool: Pool,
    pub lease_token: Option<String>,
    pub lease_node_id: Option<String>,
    pub lease_expires_at: Option<String>,
    /// Generic context carried from discover() to on_acquired().
    /// For tasks this is the agent_id; channels leave it empty.
    pub context: String,
    /// Set by the lease service on claimed entries. Call this when async work
    /// (e.g. task execution) finishes to eagerly release the lease instead of
    /// waiting for the next scan cycle to evict it.
    pub release_fn: Option<ReleaseFn>,
}

/// Trait implemented by each resource type that participates in lease management.
///
/// The lease service is a minimal coordinator: it only manages claim/renew/release
/// of lease columns. Business state transitions (e.g. task status) belong in the
/// resource implementation callbacks.
#[async_trait]
pub trait LeaseResource: Send + Sync {
    /// SQL table name (e.g. "tasks", "channel_accounts").
    fn table(&self) -> &str;

    /// Lease TTL in seconds.
    fn lease_secs(&self) -> u64;

    /// How often to scan for resources (seconds).
    fn scan_interval_secs(&self) -> u64;

    /// Discover all resources that should be lease-managed, with current DB lease state.
    async fn discover(&self) -> Result<Vec<ResourceEntry>>;

    /// Called after this instance successfully claims a resource.
    /// Return `Err` to signal that the resource could not be started;
    /// the lease service will release the DB lease and call `on_released`.
    async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()>;

    /// Called when a lease is released for ANY reason: health check failure,
    /// release_fn invocation, on_acquired failure, shutdown, or stale eviction.
    /// Use this to clean up business state (e.g. reset task status to idle).
    async fn on_released(&self, resource_id: &str, pool: &Pool);

    /// Extra WHERE condition appended to the claim UPDATE (e.g. "enabled = true").
    fn claim_condition(&self) -> Option<&str> {
        None
    }

    /// Check if a held resource is still healthy. Called before renewing.
    /// Return `false` to trigger lease release and allow re-acquisition.
    async fn is_healthy(&self, _resource_id: &str) -> bool {
        true
    }

    /// Whether it's safe to release all leases for this resource type during
    /// shutdown. Return `false` to skip release and let leases expire naturally
    /// (e.g. when async workers are still running and premature release would
    /// cause duplicate execution on another instance).
    fn safe_to_release(&self) -> bool {
        true
    }
}
