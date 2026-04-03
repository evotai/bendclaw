#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::bail;
use anyhow::Context as _;
use axum::body::to_bytes;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use bendclaw::config::BendClawConfig;
use bendclaw::storage::Pool;
use serde_json::Value;
use tower::ServiceExt;
use ulid::Ulid;

// ── TestContext: setup / teardown ────────────────────────────────────────────

pub struct TestContext {
    pool: Pool,
    prefix: String,
    db_name: String,
    /// Set to true after cleanup so Drop doesn't double-clean.
    cleaned: std::sync::atomic::AtomicBool,
}

impl TestContext {
    pub async fn setup() -> anyhow::Result<Self> {
        let (base_url, token, warehouse) = require_api_config()?;
        let root = Pool::new(&base_url, &token, &warehouse)?;

        let id = &Ulid::new().to_string().to_lowercase()[..8];
        let prefix = format!("test_bendclaw_{id}_");
        let db_name = format!("test_bendclaw_{id}");

        Ok(Self {
            pool: root,
            prefix,
            db_name,
            cleaned: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub async fn app_with_llm(
        &self,
        llm: Arc<dyn bendclaw::llm::provider::LLMProvider>,
    ) -> anyhow::Result<axum::Router> {
        use bendclaw::server::state::AppState;

        let (base_url, token, warehouse) = require_api_config()?;
        let skills_dir = std::env::temp_dir().join("bendclaw-test-skills");
        std::fs::create_dir_all(&skills_dir)?;

        let runtime = bendclaw::runtime::Runtime::new(
            &base_url,
            &token,
            &warehouse,
            &self.prefix,
            "test_instance",
            llm,
        )
        .with_hub_config(None)
        .build()
        .await?;

        let state = AppState {
            runtime,
            auth_key: String::new(),
            shutdown_token: tokio_util::sync::CancellationToken::new(),
        };
        Ok(bendclaw::server::api_router(
            state,
            "info",
            &bendclaw::config::AuthConfig::default(),
        ))
    }

    #[allow(dead_code)]
    pub async fn pool(&self) -> anyhow::Result<Pool> {
        ensure_test_db(&self.pool, &self.db_name).await
    }

    #[allow(dead_code)]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    #[allow(dead_code)]
    async fn do_teardown(&self) -> anyhow::Result<()> {
        let sql = format!("SHOW DATABASES LIKE '{}%'", self.prefix);
        let rows = self.pool.query_all(&sql).await?;
        for row in &rows {
            let name: String = col(row, 0);
            self.pool
                .exec(&format!("DROP DATABASE IF EXISTS `{name}`"))
                .await?;
        }
        // Also drop the shared db
        self.pool
            .exec(&format!("DROP DATABASE IF EXISTS `{}`", self.db_name))
            .await?;
        Ok(())
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        if self.cleaned.load(Ordering::Relaxed) {
            return;
        }
        let pool = self.pool.clone();
        let prefix = self.prefix.clone();
        let db_name = self.db_name.clone();
        let _ = std::thread::Builder::new()
            .name("bendclaw-test-cleanup".to_string())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    let sql = format!("SHOW DATABASES LIKE '{prefix}%'");
                    if let Ok(rows) = pool.query_all(&sql).await {
                        for row in &rows {
                            let name: String = col(row, 0);
                            let _ = pool
                                .exec(&format!("DROP DATABASE IF EXISTS `{name}`"))
                                .await;
                        }
                    }
                    let _ = pool
                        .exec(&format!("DROP DATABASE IF EXISTS `{db_name}`"))
                        .await;
                });
            })
            .and_then(|handle| {
                handle
                    .join()
                    .map_err(|_| std::io::Error::other("cleanup thread panicked"))
            });
    }
}

async fn ensure_test_db(pool: &Pool, db_name: &str) -> anyhow::Result<Pool> {
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
        .await?;
    let db_pool = pool.with_database(db_name)?;
    for sql in ALL_MIGRATIONS {
        run_migration(&db_pool, sql).await?;
    }
    Ok(db_pool)
}

// ── Public helpers ────────────────────────────────────────────────────────────

pub fn uid(prefix: &str) -> String {
    format!("{prefix}-{}", Ulid::new())
}

/// Return a pool connected to a per-process test database with all migrations applied.
/// Used by kernel-level tests (session, tools) that don't go through the HTTP layer.
/// The database is created once per process and cleaned up via `cleanup_pool_db`.
pub async fn pool() -> anyhow::Result<Pool> {
    static POOL: tokio::sync::OnceCell<Pool> = tokio::sync::OnceCell::const_new();
    POOL.get_or_try_init(create_pool).await.cloned()
}

async fn create_pool() -> anyhow::Result<Pool> {
    let (base_url, token, warehouse) = require_api_config()?;
    let root = Pool::new(&base_url, &token, &warehouse)?;
    let db_name = pool_db_name();

    root.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
        .await?;
    let pool = root.with_database(&db_name)?;

    for sql in ALL_MIGRATIONS {
        run_migration(&pool, sql).await?;
    }
    Ok(pool)
}

fn pool_db_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let id = &Ulid::new().to_string().to_lowercase()[..8];
        format!("test_bendclaw_{id}")
    })
    .clone()
}

pub async fn json_body(resp: axum::response::Response) -> anyhow::Result<Value> {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub async fn setup_agent(app: &axum::Router, agent_id: &str, user: &str) -> anyhow::Result<()> {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/setup"))
                .header("x-user-id", user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    Ok(())
}

pub async fn chat(
    app: &axum::Router,
    agent_id: &str,
    session_id: &str,
    user: &str,
    message: &str,
) -> anyhow::Result<Value> {
    let body = serde_json::json!({
        "session_id": session_id,
        "input": message,
        "stream": false,
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", user)
                .body(Body::from(serde_json::to_vec(&body)?))?,
        )
        .await?;
    let status = resp.status();
    let run = json_body(resp).await?;
    if status != StatusCode::OK {
        anyhow::bail!("chat request failed: status={status}, body={run}");
    }
    Ok(serde_json::json!({
        "ok": true,
        "message": run["output"],
        "run": run,
    }))
}

pub fn require_api_config() -> anyhow::Result<(String, String, String)> {
    initialize_test_env()?;
    let base_url = std::env::var("BENDCLAW_STORAGE_DATABEND_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.databend.com/v1".to_string());
    let token =
        std::env::var("BENDCLAW_STORAGE_DATABEND_API_TOKEN").unwrap_or_else(|_| String::new());
    let warehouse = std::env::var("BENDCLAW_STORAGE_DATABEND_WAREHOUSE")
        .unwrap_or_else(|_| "default".to_string());
    Ok((base_url, token, warehouse))
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn col(row: &serde_json::Value, idx: usize) -> String {
    row.as_array()
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

const ALL_MIGRATIONS: &[&str] = &[
    include_str!("../../../migrations/base/sessions.sql"),
    include_str!("../../../migrations/base/runs.sql"),
    include_str!("../../../migrations/base/agent.sql"),
    include_str!("../../../migrations/base/traces.sql"),
    include_str!("../../../migrations/base/tasks.sql"),
    include_str!("../../../migrations/base/feedback.sql"),
    include_str!("../../../migrations/base/channels.sql"),
    include_str!("../../../migrations/base/recall.sql"),
    include_str!("../../../migrations/org/agents.sql"),
    include_str!("../../../migrations/org/memory.sql"),
];

async fn run_migration(pool: &Pool, sql: &str) -> anyhow::Result<()> {
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        let has_code = stmt
            .lines()
            .any(|l| !l.trim().is_empty() && !l.trim().starts_with("--"));
        if !has_code {
            continue;
        }
        pool.exec(stmt)
            .await
            .with_context(|| format!("migration statement failed:\n{stmt}"))?;
    }
    Ok(())
}

fn dev_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("BENDCLAW_DEV_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".bendclaw")
        .join("bendclaw_dev.toml")
}

const DEV_CONFIG_TEMPLATE: &str = include_str!("../../../configs/bendclaw.toml.example");

fn initialize_test_env() -> anyhow::Result<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    let result = INIT.get_or_init(do_initialize_test_env);
    if let Err(msg) = result {
        bail!("{msg}");
    }
    Ok(())
}

fn do_initialize_test_env() -> Result<(), String> {
    if std::env::var_os("BENDCLAW_STORAGE_DATABEND_API_BASE_URL").is_some() {
        return Ok(());
    }

    let path = dev_config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, DEV_CONFIG_TEMPLATE);
    }

    match BendClawConfig::load(&path.to_string_lossy()) {
        Ok(cfg) if !cfg.storage.databend_api_base_url.is_empty() => {
            std::env::set_var(
                "BENDCLAW_STORAGE_DATABEND_API_BASE_URL",
                &cfg.storage.databend_api_base_url,
            );
            if !cfg.storage.databend_api_token.is_empty() {
                std::env::set_var(
                    "BENDCLAW_STORAGE_DATABEND_API_TOKEN",
                    &cfg.storage.databend_api_token,
                );
            }
            if !cfg.storage.databend_warehouse.is_empty() {
                std::env::set_var(
                    "BENDCLAW_STORAGE_DATABEND_WAREHOUSE",
                    &cfg.storage.databend_warehouse,
                );
            }
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "failed to load dev config {}: {e:#}",
            path.display()
        )),
    }
}
