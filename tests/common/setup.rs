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
}

impl TestContext {
    pub async fn setup() -> anyhow::Result<Self> {
        let (base_url, token, warehouse) = require_api_config()?;
        let root = Pool::new(&base_url, &token, &warehouse)?;

        let id = &Ulid::new().to_string().to_lowercase()[..8];
        let prefix = format!("test_bendclaw_{id}_");
        let db_name = format!("test_bendclaw_{id}");

        root.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
            .await?;
        let db_pool = root.with_database(&db_name)?;

        for sql in ALL_MIGRATIONS {
            run_migration(&db_pool, sql).await?;
        }

        Ok(Self {
            pool: root,
            prefix,
            db_name,
        })
    }

    pub async fn app(&self) -> anyhow::Result<axum::Router> {
        let llm = Arc::new(bendclaw::llm::router::LLMRouter::from_config(
            &bendclaw::llm::config::LLMConfig::default(),
        )?);
        self.app_with_llm(llm).await
    }

    pub async fn app_with_llm(
        &self,
        llm: Arc<dyn bendclaw::llm::provider::LLMProvider>,
    ) -> anyhow::Result<axum::Router> {
        use bendclaw::service::state::AppState;

        let (base_url, token, warehouse) = require_api_config()?;
        let skills_dir = std::env::temp_dir().join("bendclaw-test-skills");
        std::fs::create_dir_all(&skills_dir)?;

        let runtime =
            bendclaw::kernel::Runtime::new(&base_url, &token, &warehouse, &self.prefix, llm)
                .with_skills_dir(&skills_dir.to_string_lossy())
                .build()
                .await?;

        let state = AppState {
            runtime,
            auth_key: String::new(),
        };
        Ok(bendclaw::service::api_router(
            state,
            "info",
            &bendclaw::config::AuthConfig::default(),
        ))
    }

    #[allow(dead_code)]
    pub fn pool(&self) -> anyhow::Result<Pool> {
        Ok(self.pool.with_database(&self.db_name)?)
    }

    #[allow(dead_code)]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub async fn teardown(self) {
        if let Err(e) = self.do_teardown().await {
            eprintln!("teardown failed for prefix {}: {e:#}", self.prefix);
        }
    }

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

pub async fn cleanup_prefix(prefix: &str) -> anyhow::Result<()> {
    let (base_url, token, warehouse) = require_api_config()?;
    let pool = Pool::new(&base_url, &token, &warehouse)?;
    let sql = format!("SHOW DATABASES LIKE '{prefix}%'");
    let rows = pool.query_all(&sql).await?;
    for row in &rows {
        let name: String = col(row, 0);
        pool.exec(&format!("DROP DATABASE IF EXISTS `{name}`"))
            .await?;
    }
    Ok(())
}

pub fn require_api_config() -> anyhow::Result<(String, String, String)> {
    initialize_test_env()?;
    let base_url = std::env::var("BENDCLAW_STORAGE_DATABEND_API_BASE_URL")
        .unwrap_or_else(|_| "https://app.databend.com/v1.1".to_string());
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
    include_str!("../../migrations/0001_sessions_runs.sql"),
    include_str!("../../migrations/0002_agent.sql"),
    include_str!("../../migrations/0003_memory.sql"),
    include_str!("../../migrations/0004_skills.sql"),
    include_str!("../../migrations/0005_traces.sql"),
    include_str!("../../migrations/0006_variables_tasks.sql"),
    include_str!("../../migrations/0007_feedback.sql"),
    include_str!("../../migrations/0008_channels.sql"),
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

const DEV_CONFIG_TEMPLATE: &str = include_str!("../../configs/bendclaw.toml.example");

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
