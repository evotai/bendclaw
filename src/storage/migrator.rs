use tracing;

use super::pool::Pool;

const AGENT_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_sessions_runs.sql"),
    include_str!("../../migrations/0002_agent.sql"),
    include_str!("../../migrations/0003_memory.sql"),
    include_str!("../../migrations/0004_skills.sql"),
    include_str!("../../migrations/0005_traces.sql"),
    include_str!("../../migrations/0006_variables_tasks.sql"),
    include_str!("../../migrations/0007_feedback.sql"),
    include_str!("../../migrations/0008_channels.sql"),
];

/// Run all agent migrations against the pool's current database.
pub async fn run_agent(pool: &Pool) {
    run_statements(pool, AGENT_MIGRATIONS, "agent").await;
}

/// Run a list of raw SQL migrations against the current database.
/// All `CREATE TABLE IF NOT EXISTS` statements run concurrently for speed.
async fn run_statements(pool: &Pool, migrations: &[&str], scope: &str) {
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
