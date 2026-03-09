use super::record::SpanRecord;
use super::record::TraceRecord;
use super::types::AgentTraceBreakdown;
use super::types::AgentTraceDetails;
use super::types::AgentTraceSummary;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

// ── TraceRepo ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct TraceMapper;

impl RowMapper for TraceMapper {
    type Entity = TraceRecord;

    fn columns(&self) -> &str {
        "trace_id, run_id, session_id, agent_id, user_id, name, status, duration_ms, input_tokens, output_tokens, total_cost, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> TraceRecord {
        TraceRecord {
            trace_id: sql::col(row, 0),
            run_id: sql::col(row, 1),
            session_id: sql::col(row, 2),
            agent_id: sql::col(row, 3),
            user_id: sql::col(row, 4),
            name: sql::col(row, 5),
            status: sql::col(row, 6),
            duration_ms: parse_u64(&sql::col(row, 7)),
            input_tokens: parse_u64(&sql::col(row, 8)),
            output_tokens: parse_u64(&sql::col(row, 9)),
            total_cost: sql::col(row, 10).parse().unwrap_or(0.0),
            created_at: sql::col(row, 11),
            updated_at: sql::col(row, 12),
        }
    }
}

#[derive(Clone)]
pub struct TraceRepo {
    table: DatabendTable<TraceMapper>,
}

impl TraceRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "traces", TraceMapper),
        }
    }

    pub async fn insert(&self, record: &TraceRecord) -> Result<()> {
        let result = self
            .table
            .insert(&[
                ("trace_id", SqlVal::Str(&record.trace_id)),
                ("run_id", SqlVal::Str(&record.run_id)),
                ("session_id", SqlVal::Str(&record.session_id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("user_id", SqlVal::Str(&record.user_id)),
                ("name", SqlVal::Str(&record.name)),
                ("status", SqlVal::Str(&record.status)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                "traces",
                "insert",
                serde_json::json!({"trace_id": record.trace_id}),
                error,
            );
        }
        result
    }

    pub async fn update_completed(
        &self,
        trace_id: &str,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_cost: f64,
    ) -> Result<()> {
        let sql = format!(
            "UPDATE traces SET status = 'completed', duration_ms = {}, input_tokens = {}, output_tokens = {}, total_cost = {}, updated_at = NOW() WHERE trace_id = '{}'",
            duration_ms, input_tokens, output_tokens, total_cost, sql::escape(trace_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                "traces",
                "update_completed",
                serde_json::json!({"trace_id": trace_id}),
                error,
            );
        }
        result
    }

    pub async fn update_failed(&self, trace_id: &str, duration_ms: u64) -> Result<()> {
        let sql = format!(
            "UPDATE traces SET status = 'failed', duration_ms = {}, updated_at = NOW() WHERE trace_id = '{}'",
            duration_ms, sql::escape(trace_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                "traces",
                "update_failed",
                serde_json::json!({"trace_id": trace_id}),
                error,
            );
        }
        result
    }

    pub async fn load(&self, trace_id: &str) -> Result<Option<TraceRecord>> {
        let result = self
            .table
            .get(&[Where("trace_id", SqlVal::Str(trace_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                "traces",
                "load",
                serde_json::json!({"trace_id": trace_id}),
                error,
            );
        }
        result
    }

    pub async fn list_by_session(&self, session_id: &str, limit: u32) -> Result<Vec<TraceRecord>> {
        let result = self
            .table
            .list(
                &[Where("session_id", SqlVal::Str(session_id))],
                "created_at DESC",
                limit as u64,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                "traces",
                "list_by_session",
                serde_json::json!({"session_id": session_id, "limit": limit}),
                error,
            );
        }
        result
    }

    pub async fn summary_for_agent(&self, agent_id: &str) -> Result<AgentTraceSummary> {
        let aid = sql::escape(agent_id);
        let q1 = format!(
            "SELECT \
                COUNT(*) AS trace_count, \
                SUM(input_tokens) AS input_tokens, \
                SUM(output_tokens) AS output_tokens, \
                SUM(total_cost) AS total_cost, \
                AVG(duration_ms) AS avg_duration_ms, \
                MAX(TO_VARCHAR(created_at)) AS last_active \
            FROM traces WHERE agent_id = '{aid}'"
        );
        let row = self.table.pool().query_row(&q1).await?;
        let q2 = format!(
            "SELECT \
                SUM(CASE WHEN kind = 'llm' AND status = 'completed' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN kind = 'tool' AND status = 'completed' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN kind = 'skill' AND status = 'completed' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) \
            FROM spans WHERE trace_id IN (SELECT trace_id FROM traces WHERE agent_id = '{aid}')"
        );
        let sr = self.table.pool().query_row(&q2).await?;
        Ok(AgentTraceSummary {
            agent_id: agent_id.to_string(),
            trace_count: parse_i64_col(row.as_ref(), 0),
            input_tokens: parse_i64_col(row.as_ref(), 1),
            output_tokens: parse_i64_col(row.as_ref(), 2),
            total_cost: parse_f64_col(row.as_ref(), 3),
            avg_duration_ms: parse_f64_col(row.as_ref(), 4),
            last_active: parse_str_col(row.as_ref(), 5),
            llm_calls: parse_i64_col(sr.as_ref(), 0),
            tool_calls: parse_i64_col(sr.as_ref(), 1),
            skill_calls: parse_i64_col(sr.as_ref(), 2),
            error_count: parse_i64_col(sr.as_ref(), 3),
        })
    }

    pub async fn agent_details(&self, agent_id: &str) -> Result<AgentTraceDetails> {
        let summary = self.summary_for_agent(agent_id).await?;
        let aid = sql::escape(agent_id);

        let trace_ids_q = format!(
            "SELECT trace_id FROM traces WHERE agent_id = '{aid}' ORDER BY created_at DESC LIMIT 20"
        );
        let id_rows = self.table.pool().query_all(&trace_ids_q).await?;
        let recent_trace_ids: Vec<String> = id_rows
            .iter()
            .filter_map(|r| {
                r.as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        let breakdown_q = |kind: &str| {
            format!(
                "SELECT name, COUNT(*) AS calls, SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS errors, \
                 AVG(duration_ms) AS avg_ms, SUM(cost) AS total_cost \
                 FROM spans WHERE kind = '{kind}' AND trace_id IN (SELECT trace_id FROM traces WHERE agent_id = '{aid}') \
                 GROUP BY name ORDER BY calls DESC LIMIT 50"
            )
        };

        let llm = self.query_breakdowns(&breakdown_q("llm")).await?;
        let tools = self.query_breakdowns(&breakdown_q("tool")).await?;
        let skills = self.query_breakdowns(&breakdown_q("skill")).await?;

        let error_q = format!(
            "SELECT name, COUNT(*) AS calls, 0 AS errors, AVG(duration_ms) AS avg_ms, 0 AS total_cost \
             FROM spans WHERE status = 'failed' AND trace_id IN (SELECT trace_id FROM traces WHERE agent_id = '{aid}') \
             GROUP BY name ORDER BY calls DESC LIMIT 50"
        );
        let errors = self.query_breakdowns(&error_q).await?;

        Ok(AgentTraceDetails {
            agent_id: summary.agent_id,
            trace_count: summary.trace_count,
            llm_calls: summary.llm_calls,
            tool_calls: summary.tool_calls,
            skill_calls: summary.skill_calls,
            error_count: summary.error_count,
            input_tokens: summary.input_tokens,
            output_tokens: summary.output_tokens,
            total_cost: summary.total_cost,
            avg_duration_ms: summary.avg_duration_ms,
            last_active: summary.last_active,
            llm,
            tools,
            skills,
            errors,
            recent_trace_ids,
        })
    }

    async fn query_breakdowns(&self, q: &str) -> Result<Vec<AgentTraceBreakdown>> {
        let rows = self.table.pool().query_all(q).await?;
        Ok(rows
            .iter()
            .map(|r| AgentTraceBreakdown {
                name: r
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                calls: parse_i64_col(Some(r), 1),
                errors: parse_i64_col(Some(r), 2),
                avg_duration_ms: parse_f64_col(Some(r), 3),
                total_cost: parse_f64_col(Some(r), 4),
            })
            .collect())
    }
}

// ── SpanRepo ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct SpanMapper;

impl RowMapper for SpanMapper {
    type Entity = SpanRecord;

    fn columns(&self) -> &str {
        "span_id, trace_id, parent_span_id, name, kind, model_role, status, duration_ms, ttft_ms, input_tokens, output_tokens, reasoning_tokens, cost, error_code, error_message, summary, meta, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> SpanRecord {
        SpanRecord {
            span_id: sql::col(row, 0),
            trace_id: sql::col(row, 1),
            parent_span_id: sql::col(row, 2),
            name: sql::col(row, 3),
            kind: sql::col(row, 4),
            model_role: sql::col(row, 5),
            status: sql::col(row, 6),
            duration_ms: parse_u64(&sql::col(row, 7)),
            ttft_ms: parse_u64(&sql::col(row, 8)),
            input_tokens: parse_u64(&sql::col(row, 9)),
            output_tokens: parse_u64(&sql::col(row, 10)),
            reasoning_tokens: parse_u64(&sql::col(row, 11)),
            cost: sql::col(row, 12).parse().unwrap_or(0.0),
            error_code: sql::col(row, 13),
            error_message: sql::col(row, 14),
            summary: sql::col(row, 15),
            meta: sql::col(row, 16),
            created_at: sql::col(row, 17),
        }
    }
}

#[derive(Clone)]
pub struct SpanRepo {
    table: DatabendTable<SpanMapper>,
}

impl SpanRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "spans", SpanMapper),
        }
    }

    pub async fn append(&self, record: &SpanRecord) -> Result<()> {
        let result = self
            .table
            .insert(&[
                ("span_id", SqlVal::Str(&record.span_id)),
                ("trace_id", SqlVal::Str(&record.trace_id)),
                ("parent_span_id", SqlVal::Str(&record.parent_span_id)),
                ("name", SqlVal::Str(&record.name)),
                ("kind", SqlVal::Str(&record.kind)),
                ("model_role", SqlVal::Str(&record.model_role)),
                ("status", SqlVal::Str(&record.status)),
                ("duration_ms", SqlVal::Raw(&record.duration_ms.to_string())),
                ("ttft_ms", SqlVal::Raw(&record.ttft_ms.to_string())),
                (
                    "input_tokens",
                    SqlVal::Raw(&record.input_tokens.to_string()),
                ),
                (
                    "output_tokens",
                    SqlVal::Raw(&record.output_tokens.to_string()),
                ),
                (
                    "reasoning_tokens",
                    SqlVal::Raw(&record.reasoning_tokens.to_string()),
                ),
                ("cost", SqlVal::Raw(&record.cost.to_string())),
                ("error_code", SqlVal::Str(&record.error_code)),
                ("error_message", SqlVal::Str(&record.error_message)),
                ("summary", SqlVal::Str(&record.summary)),
                ("meta", SqlVal::Str(&record.meta)),
                ("created_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                "spans",
                "append",
                serde_json::json!({"span_id": record.span_id, "trace_id": record.trace_id}),
                error,
            );
        }
        result
    }

    pub async fn list_by_trace(&self, trace_id: &str) -> Result<Vec<SpanRecord>> {
        let result = self
            .table
            .list(
                &[Where("trace_id", SqlVal::Str(trace_id))],
                "created_at ASC",
                1000,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                "spans",
                "list_by_trace",
                serde_json::json!({"trace_id": trace_id}),
                error,
            );
        }
        result
    }

    pub async fn list_where(
        &self,
        condition: &str,
        order: &str,
        limit: u64,
    ) -> Result<Vec<SpanRecord>> {
        let result = self.table.list_where(condition, order, limit).await;
        if let Err(error) = &result {
            repo_error(
                "spans",
                "list_where",
                serde_json::json!({"condition": condition, "limit": limit}),
                error,
            );
        }
        result
    }
}

fn parse_u64(s: &str) -> u64 {
    s.parse().unwrap_or(0)
}

fn parse_i64_col(row: Option<&serde_json::Value>, idx: usize) -> i64 {
    row.and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn parse_f64_col(row: Option<&serde_json::Value>, idx: usize) -> f64 {
    row.and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

fn parse_str_col(row: Option<&serde_json::Value>, idx: usize) -> String {
    row.and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}
