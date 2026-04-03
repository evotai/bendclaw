use super::http::DailyQuery;
use super::http::DailyUsageResponse;
use super::http::UsageSummaryResponse;
use crate::server::error::Result;
use crate::server::state::AppState;
use crate::storage::dal::usage::repo::UsageRepo;

fn to_summary(s: crate::storage::dal::usage::CostSummary) -> UsageSummaryResponse {
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
    let repo = UsageRepo::new(pool);
    let rows = repo.daily_by_agent(agent_id, days).await?;
    Ok(rows
        .into_iter()
        .map(|row| DailyUsageResponse {
            date: row.date,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            total_tokens: row.total_tokens,
            cost: row.cost,
            requests: row.requests,
        })
        .collect())
}

pub async fn global_usage_summary(state: &AppState) -> Result<UsageSummaryResponse> {
    let agent_ids = state.runtime.databases().list_agent_ids().await?;
    let mut total = crate::storage::dal::usage::CostSummary::default();
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
