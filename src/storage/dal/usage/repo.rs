use tracing;

use super::record::UsageRecord;
use super::types::CostSummary;
use super::types::DailyUsage;
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
    fn parse(&self, _row: &serde_json::Value) -> crate::base::Result<()> {
        Ok(())
    }
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

        let result = self.table.insert_batch(columns, &rows).await;
        if let Err(error) = &result {
            tracing::error!(count = records.len(), error = %error, "usage batch save failed");
        }
        result?;
        tracing::debug!(count = records.len(), "usage batch saved");
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

    pub async fn daily_by_agent(&self, agent_id: &str, days: u32) -> Result<Vec<DailyUsage>> {
        let aid = sql::escape(agent_id);
        let query = format!(
            "SELECT TO_VARCHAR(TO_DATE(created_at)) AS day, \
             COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
             COALESCE(SUM(total_tokens), 0), COALESCE(SUM(cost), 0), COUNT(*) \
             FROM usage WHERE agent_id = '{aid}' AND created_at >= NOW() - INTERVAL {days} DAY \
             GROUP BY day ORDER BY day DESC"
        );
        let rows = self.table.pool().query_all(&query).await?;
        rows.iter().map(parse_daily_usage).collect()
    }

    async fn summary_with_condition(&self, condition: Option<&str>) -> Result<CostSummary> {
        let select = "COALESCE(SUM(prompt_tokens), 0), COALESCE(SUM(completion_tokens), 0), \
             COALESCE(SUM(reasoning_tokens), 0), COALESCE(SUM(total_tokens), 0), \
             COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0), \
             COALESCE(SUM(cost), 0), COUNT(*)";
        let row = self.table.aggregate(select, condition).await?;
        let summary = parse_cost_summary(row)?;
        tracing::info!(
            records = summary.record_count,
            total_tokens = summary.total_tokens,
            total_cost = summary.total_cost,
            "usage summary queried"
        );
        Ok(summary)
    }
}

fn parse_cost_summary(row: Option<serde_json::Value>) -> Result<CostSummary> {
    Ok(CostSummary {
        total_prompt_tokens: sql::agg_u64_or_zero(row.as_ref(), 0)?,
        total_completion_tokens: sql::agg_u64_or_zero(row.as_ref(), 1)?,
        total_reasoning_tokens: sql::agg_u64_or_zero(row.as_ref(), 2)?,
        total_tokens: sql::agg_u64_or_zero(row.as_ref(), 3)?,
        total_cache_read_tokens: sql::agg_u64_or_zero(row.as_ref(), 4)?,
        total_cache_write_tokens: sql::agg_u64_or_zero(row.as_ref(), 5)?,
        total_cost: sql::agg_f64_or_zero(row.as_ref(), 6)?,
        record_count: sql::agg_u64_or_zero(row.as_ref(), 7)?,
    })
}

fn parse_daily_usage(row: &serde_json::Value) -> Result<DailyUsage> {
    Ok(DailyUsage {
        date: sql::col(row, 0),
        prompt_tokens: sql::col_u64(row, 1)?,
        completion_tokens: sql::col_u64(row, 2)?,
        total_tokens: sql::col_u64(row, 3)?,
        cost: sql::col_f64(row, 4)?,
        requests: sql::col_u64(row, 5)?,
    })
}
