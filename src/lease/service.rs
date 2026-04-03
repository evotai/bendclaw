use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::diagnostics;
use super::types::LeaseResource;
use super::types::ResourceEntry;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::types::id::new_id;

// ── Builder ──────────────────────────────────────────────────────────────────

pub struct LeaseServiceBuilder {
    node_id: String,
    resources: Vec<Arc<dyn LeaseResource>>,
}

impl LeaseServiceBuilder {
    pub fn new(node_id: &str) -> Self {
        Self {
            node_id: node_id.to_string(),
            resources: Vec::new(),
        }
    }

    pub fn register(&mut self, resource: Arc<dyn LeaseResource>) {
        self.resources.push(resource);
    }

    /// Spawn one scan loop per registered resource. Returns a handle for
    /// querying state and performing graceful shutdown.
    pub fn spawn(self, cancel: CancellationToken) -> LeaseServiceHandle {
        let mut held_maps = Vec::new();
        let mut lease_counts = Vec::new();
        let mut handles = Vec::new();
        let mut abort_handles = Vec::new();

        for resource in &self.resources {
            let held = Arc::new(Mutex::new(HashMap::<String, HeldLease>::new()));
            let count = Arc::new(AtomicUsize::new(0));
            held_maps.push(held.clone());
            lease_counts.push(count.clone());

            let handle = spawn_scan_loop(
                self.node_id.clone(),
                resource.clone(),
                held,
                count,
                cancel.clone(),
            );
            abort_handles.push(handle.abort_handle());
            handles.push(handle);
        }

        LeaseServiceHandle {
            resources: self.resources,
            held_maps,
            lease_counts,
            handles: Mutex::new(handles),
            abort_handles,
        }
    }
}

/// Tracks a single held lease: the token we used to claim it, plus the pool
/// so we can release it on shutdown.
struct HeldLease {
    token: String,
    pool: Pool,
}

// ── Handle ───────────────────────────────────────────────────────────────────

pub struct LeaseServiceHandle {
    resources: Vec<Arc<dyn LeaseResource>>,
    held_maps: Vec<Arc<Mutex<HashMap<String, HeldLease>>>>,
    lease_counts: Vec<Arc<AtomicUsize>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    abort_handles: Vec<tokio::task::AbortHandle>,
}

impl LeaseServiceHandle {
    /// Total number of leases currently held across all resource types (sync-safe).
    pub fn active_lease_count(&self) -> usize {
        self.lease_counts
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .sum()
    }

    /// Release all DB leases held by this instance (best-effort, for shutdown).
    /// Skips resource types where `safe_to_release()` returns false.
    pub async fn release_all(&self) {
        let mut futs = Vec::new();
        for (i, resource) in self.resources.iter().enumerate() {
            if !resource.safe_to_release() {
                continue;
            }
            let held = self.held_maps[i].lock().await;
            for (id, lease) in held.iter() {
                let table = resource.table().to_string();
                let id = id.clone();
                let token = lease.token.clone();
                let pool = lease.pool.clone();
                let resource = resource.clone();
                futs.push(async move {
                    if let Err(e) = release_sql(&pool, &table, &id, &token).await {
                        diagnostics::log_lease_release_failed(&table, &id, &e);
                    }
                    resource.on_released(&id, &pool).await;
                });
            }
            self.lease_counts[i].store(0, Ordering::Relaxed);
        }
        crate::types::runtime::join_bounded(futs, crate::types::runtime::CONCURRENCY_SHUTDOWN)
            .await;
    }

    /// Wait for all scan loops to finish (call after cancellation).
    pub async fn join(&self) {
        let handles: Vec<_> = self.handles.lock().await.drain(..).collect();
        for handle in handles {
            let _ = handle.await;
        }
    }

    /// Force-abort all scan loops (use when join times out).
    pub fn abort_all(&self) {
        for ah in &self.abort_handles {
            ah.abort();
        }
    }
}

// ── Scan loop ────────────────────────────────────────────────────────────────

fn spawn_scan_loop(
    node_id: String,
    resource: Arc<dyn LeaseResource>,
    held: Arc<Mutex<HashMap<String, HeldLease>>>,
    lease_count: Arc<AtomicUsize>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    crate::types::spawn_named("lease_scan_loop", async move {
        let interval = Duration::from_secs(resource.scan_interval_secs());
        let mut consecutive_errors: u64 = 0;
        let mut prev_count: u64 = 0;

        loop {
            match scan_once(
                &node_id,
                &resource,
                &held,
                &lease_count,
                &cancel,
                &mut prev_count,
            )
            .await
            {
                Ok(()) => {
                    if consecutive_errors > 0 {
                        diagnostics::log_lease_scan_recovered(resource.table(), consecutive_errors);
                    }
                    consecutive_errors = 0;
                }
                Err(e) => {
                    consecutive_errors += 1;
                    if consecutive_errors == 1 || consecutive_errors.is_multiple_of(20) {
                        diagnostics::log_lease_scan_error(resource.table(), &e, consecutive_errors);
                    }
                }
            }

            lease_count.store(held.lock().await.len(), Ordering::Relaxed);

            let sleep_dur = if consecutive_errors > 0 {
                let max_backoff = resource.lease_secs() / 2;
                let secs = (60u64 << (consecutive_errors - 1).min(3)).min(max_backoff);
                Duration::from_secs(secs)
            } else {
                interval
            };

            tokio::select! {
                _ = cancel.cancelled() => {

                    break;
                }
                _ = tokio::time::sleep(sleep_dur) => {}
            }
        }
    })
}

async fn scan_once(
    node_id: &str,
    resource: &Arc<dyn LeaseResource>,
    held: &Arc<Mutex<HashMap<String, HeldLease>>>,
    lease_count: &Arc<AtomicUsize>,
    cancel: &CancellationToken,
    prev_count: &mut u64,
) -> crate::types::Result<()> {
    if cancel.is_cancelled() {
        return Ok(());
    }
    let scan_start = std::time::Instant::now();
    let entries = resource.discover().await?;
    let discover_ms = scan_start.elapsed().as_millis() as u64;
    let count = entries.len() as u64;
    diagnostics::log_lease_resources_discovered(resource.table(), count, discover_ms, *prev_count);
    *prev_count = count;
    let mut seen_ids = HashSet::new();
    let lease_secs = resource.lease_secs();
    let table = resource.table();
    let claim_cond = resource.claim_condition();

    let mut held_map = held.lock().await;

    for entry in &entries {
        seen_ids.insert(entry.id.clone());

        if held_map.contains_key(&entry.id) {
            // Check health before renewing.
            drop(held_map);
            let healthy = resource.is_healthy(&entry.id).await;
            held_map = held.lock().await;
            if !healthy {
                diagnostics::log_lease_unhealthy_released(table, &entry.id);
                if let Some(lease) = held_map.remove(&entry.id) {
                    let _ = release_sql(&lease.pool, table, &entry.id, &lease.token).await;
                    drop(held_map);
                    resource.on_released(&entry.id, &lease.pool).await;
                    held_map = held.lock().await;
                }
                continue;
            }
            // Concurrently released by release_fn while we checked health.
            if !held_map.contains_key(&entry.id) {
                continue;
            }
            // We hold this lease — renew it.
            let token = &held_map[&entry.id].token;
            if let Err(e) = renew_sql(&entry.pool, table, &entry.id, token, lease_secs).await {
                diagnostics::log_lease_renew_failed(table, &entry.id, &e);
                held_map.remove(&entry.id);
            }
        } else if is_held_by_other(node_id, entry) {
        } else if cancel.is_cancelled() {
            // Shutting down — don't claim new resources.
        } else {
            // Unclaimed or expired — try to claim.
            let token = new_id();
            match claim_sql(
                &entry.pool,
                table,
                &entry.id,
                node_id,
                &token,
                lease_secs,
                claim_cond,
            )
            .await
            {
                Ok(true) => {
                    diagnostics::log_lease_claimed(table, &entry.id, &entry.context, node_id);
                    held_map.insert(entry.id.clone(), HeldLease {
                        token: token.clone(),
                        pool: entry.pool.clone(),
                    });
                    // Build release callback for async workers.
                    let release_held = held.clone();
                    let release_table = table.to_string();
                    let release_count = lease_count.clone();
                    let release_resource = resource.clone();
                    let release_fn: super::types::ReleaseFn = Arc::new(move |resource_id: &str| {
                        let h = release_held.clone();
                        let t = release_table.clone();
                        let cnt = release_count.clone();
                        let res = release_resource.clone();
                        let id = resource_id.to_string();
                        crate::types::spawn_named("lease_release", async move {
                            let pool = if let Some(lease) = h.lock().await.remove(&id) {
                                let p = lease.pool.clone();
                                let _ = release_sql(&p, &t, &id, &lease.token).await;
                                Some(p)
                            } else {
                                None
                            };
                            cnt.store(h.lock().await.len(), Ordering::Relaxed);
                            if let Some(pool) = pool {
                                res.on_released(&id, &pool).await;
                            }
                        });
                    });
                    let claimed_entry = ResourceEntry {
                        id: entry.id.clone(),
                        pool: entry.pool.clone(),
                        lease_token: Some(token.clone()),
                        lease_node_id: Some(node_id.to_string()),
                        lease_expires_at: entry.lease_expires_at.clone(),
                        context: entry.context.clone(),
                        release_fn: Some(release_fn),
                    };
                    // Drop lock before callback to avoid holding it during potentially slow I/O.
                    drop(held_map);
                    if let Err(e) = resource.on_acquired(&claimed_entry).await {
                        diagnostics::log_lease_on_acquired_failed(table, &entry.id, &e);
                        let mut map = held.lock().await;
                        if let Some(lease) = map.remove(&entry.id) {
                            let _ = release_sql(&lease.pool, table, &entry.id, &lease.token).await;
                            drop(map);
                            resource.on_released(&entry.id, &lease.pool).await;
                        }
                    }
                    held_map = held.lock().await;
                }
                Ok(false) => {}
                Err(e) => {
                    diagnostics::log_lease_claim_failed(table, &entry.id, &e);
                }
            }
        }
    }

    // Evict locally held leases whose resources disappeared from discover.
    let stale: Vec<(String, Pool, String)> = held_map
        .iter()
        .filter(|(id, _)| !seen_ids.contains(*id))
        .map(|(id, lease)| (id.clone(), lease.pool.clone(), lease.token.clone()))
        .collect();
    for (id, _, _) in &stale {
        held_map.remove(id);
    }
    drop(held_map);

    for (id, pool, token) in &stale {
        let _ = release_sql(pool, table, id, token).await;
        resource.on_released(id, pool).await;
    }

    Ok(())
}

fn is_held_by_other(node_id: &str, entry: &ResourceEntry) -> bool {
    let Some(ref holder) = entry.lease_node_id else {
        return false;
    };
    if holder == node_id {
        return false;
    }
    let Some(ref expires) = entry.lease_expires_at else {
        return false;
    };
    match chrono::NaiveDateTime::parse_from_str(expires, "%Y-%m-%d %H:%M:%S%.f") {
        Ok(exp) => exp > chrono::Utc::now().naive_utc(),
        Err(_) => false,
    }
}

// ── SQL helpers ──────────────────────────────────────────────────────────────

/// Timeout for individual lease SQL operations (claim, renew).
const LEASE_SQL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Atomically claim a single resource row. Only sets lease columns.
async fn claim_sql(
    pool: &Pool,
    table: &str,
    id: &str,
    node_id: &str,
    token: &str,
    lease_secs: u64,
    extra_cond: Option<&str>,
) -> crate::types::Result<bool> {
    let mut update = sql::Sql::update(table)
        .set("lease_node_id", node_id)
        .set("lease_token", token)
        .set_raw(
            "lease_expires_at",
            &format!("ADD_SECONDS(NOW(), {lease_secs})"),
        )
        .set_raw("updated_at", "NOW()")
        .where_eq("id", id)
        .where_raw(&format!(
            "(lease_node_id IS NULL OR lease_node_id = '' \
                 OR lease_expires_at IS NULL OR lease_expires_at <= NOW() \
                 OR lease_node_id = '{}')",
            sql::escape(node_id)
        ));
    if let Some(cond) = extra_cond {
        update = update.where_raw(cond);
    }
    tokio::time::timeout(LEASE_SQL_TIMEOUT, pool.exec(&update.build()))
        .await
        .map_err(|_| crate::types::ErrorCode::timeout("claim update timed out"))??;

    let check = sql::Sql::select("COUNT(*)")
        .from(table)
        .where_eq("id", id)
        .where_eq("lease_token", token)
        .where_eq("lease_node_id", node_id)
        .build();
    let row = tokio::time::timeout(LEASE_SQL_TIMEOUT, pool.query_row(&check))
        .await
        .map_err(|_| crate::types::ErrorCode::timeout("claim check timed out"))??;
    let count = row
        .as_ref()
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(count > 0)
}

/// Renew an existing lease by extending its expiration.
async fn renew_sql(
    pool: &Pool,
    table: &str,
    id: &str,
    token: &str,
    lease_secs: u64,
) -> crate::types::Result<()> {
    let update = sql::Sql::update(table)
        .set_raw(
            "lease_expires_at",
            &format!("ADD_SECONDS(NOW(), {lease_secs})"),
        )
        .set_raw("updated_at", "NOW()")
        .where_eq("id", id)
        .where_eq("lease_token", token)
        .build();
    tokio::time::timeout(LEASE_SQL_TIMEOUT, pool.exec(&update))
        .await
        .map_err(|_| crate::types::ErrorCode::timeout("renew timed out"))?
}

/// Release a single lease. Only clears lease columns.
async fn release_sql(pool: &Pool, table: &str, id: &str, token: &str) -> crate::types::Result<()> {
    let update = sql::Sql::update(table)
        .set_raw("lease_node_id", "NULL")
        .set_raw("lease_token", "NULL")
        .set_raw("lease_expires_at", "NULL")
        .set_raw("updated_at", "NOW()")
        .where_eq("id", id)
        .where_eq("lease_token", token)
        .build();
    pool.exec(&update).await
}
