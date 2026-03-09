use tracing;

use super::record::UsageRecord;
use super::types::CostSummary;
use crate::base::Result;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;

#[derive(Clone)]
struct UsageMapper;

impl RowMapper for UsageMapper {
    type Entity = ();
    fn columns(&self) -> &str {
        "id"
    }
    fn parse(&self, _row: &serde_json::Value) {}
}

#[derive(Clone)]
pub struct UsageRepo {
    table: DatabendTable<UsageMapper>,
}

pub type UsageStore = UsageRepo;

impl UsageRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "usage", UsageMapper),
        }
    }

    pub async fn save_batch(&self, records: &[UsageRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let columns = &[
            "id",
            "agent_id",
            "user_id",
            "session_id",
            "run_id",
            "provider",
            "model",
            "model_role",
            "prompt_tokens",
            "completion_tokens",
            "reasoning_tokens",
            "total_tokens",
            "cache_read_tokens",
            "cache_write_tokens",
            "ttft_ms",
            "cost",
            "created_at",
        ];

        let rows: Vec<Vec<SqlVal<'_>>> = records
            .iter()
            .map(|r| {
                vec![
                    SqlVal::Str(&r.id),
                    SqlVal::Str(&r.agent_id),
                    SqlVal::Str(&r.user_id),
                    SqlVal::Str(&r.session_id),
                    SqlVal::Str(&r.run_id),
                    SqlVal::Str(&r.provider),
                    SqlVal::Str(&r.model),
                    SqlVal::Str(&r.model_role),
                    SqlVal::Int(r.prompt_tokens as i64),
                    SqlVal::Int(r.completion_tokens as i64),
                    SqlVal::Int(r.reasoning_tokens as i64),
                    SqlVal::Int(r.total_tokens as i64),
                    SqlVal::Int(r.cache_read_tokens as i64),
                    SqlVal::Int(r.cache_write_tokens as i64),
                    SqlVal::Int(r.ttft_ms as i64),
                    SqlVal::Float(r.cost),
                    SqlVal::Str(&r.created_at),
                ]
            })
            .collect();

        self.table.insert_batch(columns, &rows).await.map_err(|e| {
            tracing::error!(count = records.len(), error = %e, "usage batch save failed");
            e
        })?;
        tracing::info!(count = records.len(), "usage batch saved");
        Ok(())
    }

    pub async fn summary_by_user(&self, user_id: &str) -> Result<CostSummary> {
        self.summary_with_condition(Some(&format!("user_id = '{}'", sql::escape(user_id))))
            .await
    }

    pub async fn summary_by_agent(&self, agent_id: &str) -> Result<CostSummary> {
        self.summary_with_condition(Some(&format!("agent_id = '{}'", sql::escape(agent_id))))
            .await
    }

    pub async fn summary_by_agent_day(&self, agent_id: &str, day: &str) -> Result<CostSummary> {
        self.summary_with_condition(Some(&format!(
            "agent_id = '{}' AND TO_DATE(created_at) = TO_DATE('{}')",
            sql::escape(agent_id),
            sql::escape(day)
        )))
        .await
    }

    pub async fn sum_tokens_by_agent(&self, agent_id: &str) -> Result<u64> {
        Ok(self.summary_by_agent(agent_id).await?.total_tokens)
    }

    pub async fn sum_tokens_by_agent_day(&self, agent_id: &str, day: &str) -> Result<u64> {
        Ok(self.summary_by_agent_day(agent_id, day).await?.total_tokens)
    }

    async fn summary_with_condition(&self, condition: Option<&str>) -> Result<CostSummary> {
        let select = "COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
             COALESCE(SUM(reasoning_tokens), 0), COALESCE(SUM(total_tokens), 0), \
             COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0), \
             COALESCE(SUM(cost), 0), COUNT(*)";
        let row = self.table.aggregate(select, condition).await?;
        let summary = parse_cost_summary(row);
        tracing::info!(
            records = summary.record_count,
            total_tokens = summary.total_tokens,
            total_cost = summary.total_cost,
            "usage summary queried"
        );
        Ok(summary)
    }
}

fn parse_cost_summary(row: Option<serde_json::Value>) -> CostSummary {
    CostSummary {
        total_prompt_tokens: parse_u64_col(row.clone(), 0),
        total_completion_tokens: parse_u64_col(row.clone(), 1),
        total_reasoning_tokens: parse_u64_col(row.clone(), 2),
        total_tokens: parse_u64_col(row.clone(), 3),
        total_cache_read_tokens: parse_u64_col(row.clone(), 4),
        total_cache_write_tokens: parse_u64_col(row.clone(), 5),
        total_cost: parse_f64_col(row.clone(), 6),
        record_count: parse_u64_col(row, 7),
    }
}

fn parse_u64_col(row: Option<serde_json::Value>, idx: usize) -> u64 {
    row.as_ref()
        .and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn parse_f64_col(row: Option<serde_json::Value>, idx: usize) -> f64 {
    row.as_ref()
        .and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}
