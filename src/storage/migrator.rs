use tracing;

use super::pool::Pool;
use crate::base::ErrorCode;
use crate::base::Result;

const ROOT_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_sessions.sql"),
    include_str!("../../migrations/0004_memories.sql"),
    include_str!("../../migrations/0006_agent_config.sql"),
    include_str!("../../migrations/0008_skills_skill_files.sql"),
    include_str!("../../migrations/0010_usage.sql"),
];

const AGENT_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_sessions.sql"),
    include_str!("../../migrations/0002_runs.sql"),
    include_str!("../../migrations/0003_run_events.sql"),
    include_str!("../../migrations/0004_memories.sql"),
    include_str!("../../migrations/0005_learnings.sql"),
    include_str!("../../migrations/0006_agent_config.sql"),
    include_str!("../../migrations/0007_agent_config_versions.sql"),
    include_str!("../../migrations/0008_skills_skill_files.sql"),
    include_str!("../../migrations/0009_traces_spans.sql"),
    include_str!("../../migrations/0010_usage.sql"),
    include_str!("../../migrations/0011_variables.sql"),
    include_str!("../../migrations/0012_tasks.sql"),
    include_str!("../../migrations/0013_feedback.sql"),
    include_str!("../../migrations/0014_task_history.sql"),
];

/// Run root migrations against the pool's current database.
pub async fn run_root(pool: &Pool) {
    run_statements(pool, ROOT_MIGRATIONS, "root").await;
}

/// Run all agent migrations against the pool's current database.
pub async fn run_agent(pool: &Pool) {
    run_statements(pool, AGENT_MIGRATIONS, "agent").await;
}

/// Run migrations against a specific database.
/// Creates the database if it doesn't exist, then executes SQL statements in order.
pub async fn run(pool: &Pool, database: &str, migrations: &[&str]) -> Result<()> {
    // Create database if not exists
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS {database}"))
        .await?;

    let db_pool = pool.with_database(database)?;

    for (i, sql) in migrations.iter().enumerate() {
        tracing::debug!(database, index = i, "running migration");
        db_pool.exec(sql).await.map_err(|e| {
            ErrorCode::storage_migration(format!(
                "migration {i} failed for database '{database}': {e}"
            ))
        })?;
    }

    tracing::info!(database, count = migrations.len(), "migrations completed");
    Ok(())
}

/// Run a list of raw SQL migrations against the current database.
/// All `CREATE TABLE IF NOT EXISTS` statements run concurrently for speed.
pub async fn run_statements(pool: &Pool, migrations: &[&str], scope: &str) {
    let mut tasks = Vec::new();
    for sql in migrations {
        for stmt in sql.split(';').filter(|s| !s.trim().is_empty()) {
            let pool = pool.clone();
            let stmt = stmt.trim().to_string();
            tasks.push(tokio::spawn(async move { pool.exec(&stmt).await }));
        }
    }
    for task in tasks {
        if let Ok(Err(e)) = task.await {
            tracing::info!(scope, error = %e, "migration statement skipped (may already exist)");
        }
    }
    tracing::info!(scope, count = migrations.len(), "migrations completed");
}
