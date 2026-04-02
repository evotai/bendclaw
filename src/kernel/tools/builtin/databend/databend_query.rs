//! Databend tool implementation.

use async_trait::async_trait;
use serde_json::json;

use super::action::Action;
use crate::base::truncate_bytes_on_char_boundary;
use crate::base::Result;
use crate::kernel::tools::tool_context::ToolContext;
use crate::kernel::tools::tool_contract::OperationClassifier;
use crate::kernel::tools::tool_contract::Tool;
use crate::kernel::tools::tool_contract::ToolResult;
use crate::kernel::tools::tool_id::ToolId;
use crate::kernel::Impact;
use crate::kernel::OpType;
use crate::storage::Pool;

/// Maximum rows returned before truncation.
const MAX_RESULT_ROWS: usize = 1000;
/// Maximum output size in bytes (1 MB).
const MAX_OUTPUT_BYTES: usize = 1_048_576;

/// Execute SQL queries and inspect Databend database objects.
pub struct DatabendTool {
    pool: Pool,
}

impl DatabendTool {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

// ─── Operation classification ─────────────────────────────────────────────────

impl OperationClassifier for DatabendTool {
    fn op_type(&self) -> OpType {
        OpType::Databend
    }

    fn classify_impact(&self, args: &serde_json::Value) -> Option<Impact> {
        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
        match action {
            "show_databases" | "show_tables" | "show_stages" | "show_functions" | "describe" => {
                Some(Impact::Low)
            }
            "query" | "exec" => {
                let sql = args.get("sql").and_then(|v| v.as_str()).unwrap_or("");
                Some(classify_sql(sql))
            }
            _ => Some(Impact::Medium),
        }
    }

    fn summarize(&self, args: &serde_json::Value) -> String {
        match Action::parse(args) {
            Ok(action) => truncate(&action.to_sql(), 120),
            Err(_) => "databend".into(),
        }
    }
}

// ─── Tool trait ───────────────────────────────────────────────────────────────

#[async_trait]
impl Tool for DatabendTool {
    fn name(&self) -> &str {
        ToolId::Databend.as_str()
    }

    fn description(&self) -> &str {
        "Execute SQL queries and manage Databend databases, tables, stages, and functions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "query", "exec", "describe",
                        "show_databases", "show_tables", "show_stages", "show_functions"
                    ],
                    "description": "Operation: query (returns rows), exec (DDL/DML), or convenience shortcuts"
                },
                "sql": {
                    "type": "string",
                    "description": "SQL statement (required for query/exec)"
                },
                "table": {
                    "type": "string",
                    "description": "Table name (for describe)"
                },
                "database": {
                    "type": "string",
                    "description": "Database name (for describe/show_tables)"
                },
                "filter": {
                    "type": "string",
                    "description": "LIKE pattern (for show_tables/show_functions)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute_with_context(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolResult> {
        let action = match Action::parse(&args) {
            Ok(a) => a,
            Err(e) => return Ok(ToolResult::error(e)),
        };

        let sql = action.to_sql();

        if action.returns_rows() {
            match self.pool.query_all(&sql).await {
                Ok(rows) => Ok(ToolResult::ok(format_rows(&rows))),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        } else {
            match self.pool.exec(&sql).await {
                Ok(()) => Ok(ToolResult::ok("OK")),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn classify_sql(sql: &str) -> Impact {
    let sql = sql.trim_start();
    let upper = sql.to_ascii_uppercase();
    if upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESCRIBE")
        || upper.starts_with("EXPLAIN")
    {
        Impact::Low
    } else if upper.starts_with("WITH") {
        classify_with_sql(sql)
    } else if upper.starts_with("DROP") || upper.starts_with("TRUNCATE") {
        Impact::High
    } else {
        Impact::Medium
    }
}

fn classify_with_sql(sql: &str) -> Impact {
    // Minimal CTE tail detection: once we've closed CTE parens, classify from the
    // first statement keyword at depth 0.
    let mut depth = 0i32;
    let mut seen_open = false;
    let mut token_start: Option<usize> = None;

    for (i, ch) in sql.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                seen_open = true;
                token_start = None;
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                token_start = None;
            }
            _ => {
                if depth == 0 && seen_open {
                    if ch.is_ascii_alphabetic() {
                        if token_start.is_none() {
                            token_start = Some(i);
                        }
                    } else if let Some(start) = token_start {
                        if is_statement_start(&sql[start..i]) {
                            return classify_sql(&sql[start..]);
                        }
                        token_start = None;
                    }
                }
            }
        }
    }

    if let Some(start) = token_start {
        if is_statement_start(&sql[start..]) {
            return classify_sql(&sql[start..]);
        }
    }

    Impact::Medium
}

fn is_statement_start(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "SELECT"
            | "SHOW"
            | "DESCRIBE"
            | "EXPLAIN"
            | "WITH"
            | "INSERT"
            | "UPDATE"
            | "DELETE"
            | "MERGE"
            | "COPY"
            | "CREATE"
            | "ALTER"
            | "DROP"
            | "TRUNCATE"
    )
}

fn format_rows(rows: &[serde_json::Value]) -> String {
    let truncated = rows.len() > MAX_RESULT_ROWS;
    let slice = if truncated {
        &rows[..MAX_RESULT_ROWS]
    } else {
        rows
    };

    let mut output = serde_json::to_string(slice).unwrap_or_default();
    if truncated {
        output.push_str(&format!(
            "\n... truncated ({MAX_RESULT_ROWS} of {} rows)",
            rows.len()
        ));
    }
    if output.len() > MAX_OUTPUT_BYTES {
        output = truncate_bytes_on_char_boundary(&output, MAX_OUTPUT_BYTES);
        output.push_str("\n... [output truncated at 1MB]");
    }
    output
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        let end = s.floor_char_boundary(max - 3);
        format!("{}...", &s[..end])
    } else {
        s.to_string()
    }
}
