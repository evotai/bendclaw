//! Memory hygiene — automated cleanup of stale memories.

use crate::kernel::memory::diagnostics;
use crate::kernel::memory::store::MemoryStore;
use crate::types::Result;

/// Default: prune memories not accessed in 30 days with fewer than 2 accesses.
const DEFAULT_MAX_AGE_DAYS: u32 = 30;
const DEFAULT_MIN_ACCESS: u32 = 2;

/// Run hygiene cleanup with default thresholds.
pub async fn run_default(store: &dyn MemoryStore, user_id: &str) -> Result<usize> {
    run(store, user_id, DEFAULT_MAX_AGE_DAYS, DEFAULT_MIN_ACCESS).await
}

/// Run hygiene cleanup with custom thresholds.
pub async fn run(
    store: &dyn MemoryStore,
    user_id: &str,
    max_age_days: u32,
    min_access: u32,
) -> Result<usize> {
    let pruned = store.prune(user_id, max_age_days, min_access).await?;
    diagnostics::log_hygiene(user_id, pruned);
    Ok(pruned)
}
