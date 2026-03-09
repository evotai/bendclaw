use super::http::DailyQuery;
use super::http::DailyUsageResponse;
use super::http::UsageSummaryResponse;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::storage::dal::usage::repo::UsageRepo;
use crate::storage::sql;

fn to_summary(s: crate::storage::CostSummary) -> UsageSummaryResponse {
    UsageSummaryResponse {
        total_prompt_tokens: s.total_prompt_tokens,
        total_completion_tokens: s.total_completion_tokens,
        total_reasoning_tokens: s.total_reasoning_tokens,
        total_tokens: s.total_tokens,
        total_cost: s.total_cost,
        record_count: s.record_count,
        total_cache_read_tokens: s.total_cache_read_tokens,
        total_cache_write_tokens: s.total_cache_write_tokens,
    }
}

pub async fn usage_summary(state: &AppState, agent_id: &str) -> Result<UsageSummaryResponse> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = UsageRepo::new(pool);
    let s = repo.summary_by_agent(agent_id).await?;
    Ok(to_summary(s))
}

pub async fn usage_daily(
    state: &AppState,
    agent_id: &str,
    q: DailyQuery,
) -> Result<Vec<DailyUsageResponse>> {
    let days = q.days.unwrap_or(14).min(90);
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let aid = sql::escape(agent_id);
    let sql_str = format!(
        "SELECT TO_VARCHAR(TO_DATE(created_at)) AS day, \
         COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
         COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost), 0), COUNT(*) \
         FROM usage WHERE agent_id = '{aid}' AND created_at >= NOW() - INTERVAL {days} DAY \
         GROUP BY day ORDER BY day DESC"
    );
    let rows = pool.query_all(&sql_str).await?;
    Ok(rows
        .iter()
        .map(|r| DailyUsageResponse {
            date: sql::col(r, 0),
            prompt_tokens: sql::col(r, 1).parse().unwrap_or(0),
            completion_tokens: sql::col(r, 2).parse().unwrap_or(0),
            total_tokens: sql::col(r, 3).parse().unwrap_or(0),
            cost: sql::col(r, 4).parse().unwrap_or(0.0),
            requests: sql::col(r, 5).parse().unwrap_or(0),
        })
        .collect())
}

pub async fn global_usage_summary(state: &AppState) -> Result<UsageSummaryResponse> {
    let agent_ids = state.runtime.databases().list_agent_ids().await?;
    let mut total = crate::storage::CostSummary::default();
    for agent_id in &agent_ids {
        if let Ok(pool) = state.runtime.databases().agent_pool(agent_id) {
            let repo = UsageRepo::new(pool);
            if let Ok(s) = repo.summary_by_agent(agent_id).await {
                total.total_prompt_tokens += s.total_prompt_tokens;
                total.total_completion_tokens += s.total_completion_tokens;
                total.total_reasoning_tokens += s.total_reasoning_tokens;
                total.total_tokens += s.total_tokens;
                total.total_cost += s.total_cost;
                total.record_count += s.record_count;
                total.total_cache_read_tokens += s.total_cache_read_tokens;
                total.total_cache_write_tokens += s.total_cache_write_tokens;
            }
        }
    }
    Ok(to_summary(total))
}
