use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use bendclaw::lease::LeaseResource;
use bendclaw::lease::LeaseServiceBuilder;
use bendclaw::lease::ResourceEntry;
use bendclaw::storage::Pool;
use bendclaw::types::Result;
use tokio_util::sync::CancellationToken;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

// ── Test helpers ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Acquired(String),
    Released(String),
}

struct FakeResource {
    entries: Mutex<Vec<ResourceEntry>>,
    events: Arc<Mutex<Vec<Event>>>,
    fail_on_acquired: Mutex<bool>,
    pool: Pool,
}

impl FakeResource {
    fn new(pool: Pool) -> Arc<Self> {
        Arc::new(Self {
            entries: Mutex::new(Vec::new()),
            events: Arc::new(Mutex::new(Vec::new())),
            fail_on_acquired: Mutex::new(false),
            pool,
        })
    }

    fn set_entries(&self, entries: Vec<ResourceEntry>) {
        *self.entries.lock().unwrap() = entries;
    }

    fn set_fail_on_acquired(&self, fail: bool) {
        *self.fail_on_acquired.lock().unwrap() = fail;
    }

    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }

    fn entry(&self, id: &str) -> ResourceEntry {
        ResourceEntry {
            id: id.to_string(),
            pool: self.pool.clone(),
            lease_token: None,
            lease_node_id: None,
            lease_expires_at: None,
            context: String::new(),
            release_fn: None,
        }
    }

    fn entry_held_by(&self, id: &str, instance: &str, expires: &str) -> ResourceEntry {
        ResourceEntry {
            id: id.to_string(),
            pool: self.pool.clone(),
            lease_token: Some("old-token".to_string()),
            lease_node_id: Some(instance.to_string()),
            lease_expires_at: Some(expires.to_string()),
            context: String::new(),
            release_fn: None,
        }
    }
}

#[async_trait]
impl LeaseResource for FakeResource {
    fn table(&self) -> &str {
        "fake_resources"
    }
    fn lease_secs(&self) -> u64 {
        60
    }
    fn scan_interval_secs(&self) -> u64 {
        1
    }

    async fn discover(&self) -> Result<Vec<ResourceEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .iter()
            .map(|e| ResourceEntry {
                id: e.id.clone(),
                pool: e.pool.clone(),
                lease_token: e.lease_token.clone(),
                lease_node_id: e.lease_node_id.clone(),
                lease_expires_at: e.lease_expires_at.clone(),
                context: String::new(),
                release_fn: None,
            })
            .collect())
    }

    async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
        if *self.fail_on_acquired.lock().unwrap() {
            return Err(bendclaw::types::ErrorCode::internal("forced failure"));
        }
        self.events
            .lock()
            .unwrap()
            .push(Event::Acquired(entry.id.clone()));
        Ok(())
    }

    async fn on_released(&self, resource_id: &str, _pool: &Pool) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Released(resource_id.to_string()));
    }
}

fn fake_pool() -> Pool {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.starts_with("SELECT COUNT(*)") {
            return Ok(paged_rows(&[&["1"]], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    fake.pool()
}

fn fake_pool_claim_fails() -> Pool {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.starts_with("SELECT COUNT(*)") {
            return Ok(paged_rows(&[&["0"]], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    fake.pool()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn scan_claims_unclaimed_resource_and_fires_on_acquired() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("res-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    assert_eq!(resource.events(), vec![Event::Acquired(
        "res-1".to_string()
    )]);
    assert_eq!(handle.active_lease_count(), 1);
}

#[tokio::test]
async fn scan_evicts_stale_resource_and_fires_on_released() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("res-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);

    resource.set_entries(vec![]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    cancel.cancel();

    let events = resource.events();
    assert_eq!(events, vec![
        Event::Acquired("res-1".to_string()),
        Event::Released("res-1".to_string()),
    ]);
    assert_eq!(handle.active_lease_count(), 0);
}

#[tokio::test]
async fn on_acquired_failure_releases_lease_and_calls_on_released() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_fail_on_acquired(true);
    resource.set_entries(vec![resource.entry("res-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    let events = resource.events();
    assert_eq!(events, vec![Event::Released("res-1".to_string())]);
    assert_eq!(handle.active_lease_count(), 0);
}

#[tokio::test]
async fn skips_resource_held_by_other_instance() {
    let pool = fake_pool_claim_fails();
    let resource = FakeResource::new(pool);
    let future = chrono::Utc::now() + chrono::Duration::minutes(5);
    let expires = future.format("%Y-%m-%d %H:%M:%S").to_string();
    resource.set_entries(vec![resource.entry_held_by(
        "res-1",
        "other-inst",
        &expires,
    )]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    assert!(resource.events().is_empty());
    assert_eq!(handle.active_lease_count(), 0);
}

#[tokio::test]
async fn reclaims_own_expired_lease_after_restart() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry_held_by(
        "res-1",
        "inst-1",
        "2020-01-01 00:00:00",
    )]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    assert_eq!(resource.events(), vec![Event::Acquired(
        "res-1".to_string()
    )]);
    assert_eq!(handle.active_lease_count(), 1);
}

#[tokio::test]
async fn reclaims_own_valid_lease_after_restart() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    let future = chrono::Utc::now() + chrono::Duration::minutes(5);
    let expires = future.format("%Y-%m-%d %H:%M:%S").to_string();
    resource.set_entries(vec![resource.entry_held_by("res-1", "inst-1", &expires)]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    assert_eq!(resource.events(), vec![Event::Acquired(
        "res-1".to_string()
    )]);
    assert_eq!(handle.active_lease_count(), 1);
}

#[tokio::test]
async fn release_all_clears_held_leases_and_calls_on_released() {
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("res-1"), resource.entry("res-2")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 2);

    handle.release_all().await;
    assert_eq!(
        handle.active_lease_count(),
        0,
        "counter must be 0 after release_all"
    );

    let events = resource.events();
    let released_count = events
        .iter()
        .filter(|e| matches!(e, Event::Released(_)))
        .count();
    assert_eq!(
        released_count, 2,
        "on_released must be called for each held lease"
    );
    cancel.cancel();
}

#[tokio::test]
async fn claimed_entry_has_correct_lease_token() {
    let pool = fake_pool();
    let tokens: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));

    struct TokenCapture {
        inner: Arc<FakeResource>,
        tokens: Arc<Mutex<Vec<Option<String>>>>,
    }

    #[async_trait]
    impl LeaseResource for TokenCapture {
        fn table(&self) -> &str {
            self.inner.table()
        }
        fn lease_secs(&self) -> u64 {
            self.inner.lease_secs()
        }
        fn scan_interval_secs(&self) -> u64 {
            self.inner.scan_interval_secs()
        }
        async fn discover(&self) -> Result<Vec<ResourceEntry>> {
            self.inner.discover().await
        }
        async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
            self.tokens.lock().unwrap().push(entry.lease_token.clone());
            Ok(())
        }
        async fn on_released(&self, id: &str, pool: &Pool) {
            self.inner.on_released(id, pool).await
        }
    }

    let inner = FakeResource::new(pool);
    inner.set_entries(vec![inner.entry("res-1")]);

    let wrapper = Arc::new(TokenCapture {
        inner,
        tokens: tokens.clone(),
    });

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(wrapper);
    builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();

    let captured = tokens.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let token = captured[0].as_deref().expect("should have a token");
    assert!(!token.is_empty(), "claimed token must not be empty");
}

#[tokio::test]
async fn unhealthy_resource_gets_released_and_reacquired() {
    let pool = fake_pool();
    let healthy_flag = Arc::new(std::sync::atomic::AtomicBool::new(true));

    struct HealthCheckResource {
        inner: Arc<FakeResource>,
        healthy: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl LeaseResource for HealthCheckResource {
        fn table(&self) -> &str {
            self.inner.table()
        }
        fn lease_secs(&self) -> u64 {
            self.inner.lease_secs()
        }
        fn scan_interval_secs(&self) -> u64 {
            self.inner.scan_interval_secs()
        }
        async fn discover(&self) -> Result<Vec<ResourceEntry>> {
            self.inner.discover().await
        }
        async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
            self.inner.on_acquired(entry).await
        }
        async fn on_released(&self, id: &str, pool: &Pool) {
            self.inner.on_released(id, pool).await
        }
        async fn is_healthy(&self, _id: &str) -> bool {
            self.healthy.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    let inner = FakeResource::new(pool);
    inner.set_entries(vec![inner.entry("res-1")]);

    let wrapper = Arc::new(HealthCheckResource {
        inner: inner.clone(),
        healthy: healthy_flag.clone(),
    });

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(wrapper);
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);

    healthy_flag.store(false, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    cancel.cancel();

    let events = inner.events();
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Acquired(_)))
            .count()
            >= 2,
        "unhealthy resource should be released and re-acquired: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Released(_))),
        "on_released must be called for unhealthy resource: {events:?}"
    );
}

#[tokio::test]
async fn release_fn_updates_counter_and_calls_on_released() {
    let pool = fake_pool();

    struct ReleaseFnCapture {
        inner: Arc<FakeResource>,
        release_fns: Arc<Mutex<Vec<bendclaw::lease::ReleaseFn>>>,
    }

    #[async_trait]
    impl LeaseResource for ReleaseFnCapture {
        fn table(&self) -> &str {
            self.inner.table()
        }
        fn lease_secs(&self) -> u64 {
            self.inner.lease_secs()
        }
        fn scan_interval_secs(&self) -> u64 {
            60
        }
        async fn discover(&self) -> Result<Vec<ResourceEntry>> {
            self.inner.discover().await
        }
        async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
            if let Some(ref f) = entry.release_fn {
                self.release_fns.lock().unwrap().push(f.clone());
            }
            Ok(())
        }
        async fn on_released(&self, id: &str, pool: &Pool) {
            self.inner.on_released(id, pool).await
        }
    }

    let inner = FakeResource::new(pool);
    inner.set_entries(vec![inner.entry("res-1")]);
    let release_fns: Arc<Mutex<Vec<bendclaw::lease::ReleaseFn>>> = Arc::new(Mutex::new(Vec::new()));

    let wrapper = Arc::new(ReleaseFnCapture {
        inner: inner.clone(),
        release_fns: release_fns.clone(),
    });

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(wrapper);
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);

    let fns = release_fns.lock().unwrap().clone();
    assert_eq!(fns.len(), 1);
    fns[0]("res-1");

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        handle.active_lease_count(),
        0,
        "counter must update after release_fn"
    );

    let events = inner.events();
    assert!(
        events
            .iter()
            .any(|e| *e == Event::Released("res-1".to_string())),
        "release_fn must trigger on_released: {events:?}"
    );
    cancel.cancel();
}

#[tokio::test]
async fn stale_eviction_issues_release_sql_and_on_released() {
    let sql_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = sql_log.clone();
    let fake = FakeDatabend::new(move |sql, _db| {
        log_clone.lock().unwrap().push(sql.to_string());
        if sql.starts_with("SELECT COUNT(*)") {
            return Ok(paged_rows(&[&["1"]], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let pool = fake.pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("res-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);

    sql_log.lock().unwrap().clear();
    resource.set_entries(vec![]);
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    cancel.cancel();

    let sqls = sql_log.lock().unwrap();
    let has_release = sqls.iter().any(|s| {
        s.contains("UPDATE fake_resources SET")
            && s.contains("lease_node_id = NULL")
            && s.contains("lease_token = NULL")
    });
    assert!(
        has_release,
        "stale eviction must call release_sql: {sqls:?}"
    );

    let events = resource.events();
    assert!(
        events
            .iter()
            .any(|e| *e == Event::Released("res-1".to_string())),
        "stale eviction must call on_released: {events:?}"
    );
}

#[tokio::test]
async fn cancel_prevents_claiming_new_resources() {
    // After cancel, new resources appearing in discover must NOT be claimed.
    let pool = fake_pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("res-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    // First scan claims res-1.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);
    assert_eq!(resource.events(), vec![Event::Acquired(
        "res-1".to_string()
    )]);

    // Cancel, then add a new resource.
    cancel.cancel();
    resource.set_entries(vec![resource.entry("res-1"), resource.entry("res-2")]);

    // Wait for scan loop to exit.
    handle.join().await;

    // res-2 must NOT have been claimed after cancel.
    let events = resource.events();
    let acquired_ids: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Acquired(id) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        acquired_ids,
        vec!["res-1"],
        "no new claims after cancel: {events:?}"
    );
}

#[tokio::test]
async fn disabled_resource_evicted_and_released_on_next_scan() {
    // Simulates a channel account being disabled: it appears in the first
    // discover but not the second. The lease service must evict it, clear
    // the DB lease, and call on_released (which stops the receiver).
    let sql_log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = sql_log.clone();
    let fake = FakeDatabend::new(move |sql, _db| {
        log_clone.lock().unwrap().push(sql.to_string());
        if sql.starts_with("SELECT COUNT(*)") {
            return Ok(paged_rows(&[&["1"]], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let pool = fake.pool();
    let resource = FakeResource::new(pool);
    resource.set_entries(vec![resource.entry("acct-1")]);

    let cancel = CancellationToken::new();
    let mut builder = LeaseServiceBuilder::new("inst-1");
    builder.register(resource.clone());
    let handle = builder.spawn(cancel.clone());

    // First scan claims acct-1 (receiver started).
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(handle.active_lease_count(), 1);

    // "Disable" the account — remove from discover results.
    sql_log.lock().unwrap().clear();
    resource.set_entries(vec![]);

    // Next scan evicts it.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    cancel.cancel();

    assert_eq!(handle.active_lease_count(), 0);

    // DB lease must be cleared.
    let sqls = sql_log.lock().unwrap();
    let has_release = sqls
        .iter()
        .any(|s| s.contains("UPDATE fake_resources SET") && s.contains("lease_node_id = NULL"));
    assert!(
        has_release,
        "disabled resource must have DB lease cleared: {sqls:?}"
    );

    // on_released must fire (supervisor.stop equivalent).
    let events = resource.events();
    assert_eq!(events, vec![
        Event::Acquired("acct-1".to_string()),
        Event::Released("acct-1".to_string()),
    ]);
}
