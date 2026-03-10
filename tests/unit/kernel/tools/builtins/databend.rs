use anyhow::Result;
use bendclaw::kernel::tools::databend::DatabendTool;
use bendclaw::kernel::tools::Impact;
use bendclaw::kernel::tools::OpType;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolId;
use serde_json::json;

// ── Action parsing & SQL generation ──────────────────────────────────────────

mod action {
    use bendclaw::kernel::tools::databend::action::Action;
    use serde_json::json;

    #[test]
    fn parse_query() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "query", "sql": "SELECT 1"}))?;
        assert_eq!(action.to_sql(), "SELECT 1");
        assert!(action.returns_rows());
        Ok(())
    }

    #[test]
    fn parse_exec() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "exec", "sql": "CREATE TABLE t(id INT)"}))?;
        assert_eq!(action.to_sql(), "CREATE TABLE t(id INT)");
        assert!(!action.returns_rows());
        Ok(())
    }

    #[test]
    fn parse_describe_without_database() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "describe", "table": "users"}))?;
        assert_eq!(action.to_sql(), "DESCRIBE `users`");
        assert!(action.returns_rows());
        Ok(())
    }

    #[test]
    fn parse_describe_with_database() -> Result<(), Box<dyn std::error::Error>> {
        let action =
            Action::parse(&json!({"action": "describe", "table": "users", "database": "mydb"}))?;
        assert_eq!(action.to_sql(), "DESCRIBE `mydb`.`users`");
        Ok(())
    }

    #[test]
    fn parse_show_databases() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_databases"}))?;
        assert_eq!(action.to_sql(), "SHOW DATABASES");
        assert!(action.returns_rows());
        Ok(())
    }

    #[test]
    fn parse_show_tables_plain() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_tables"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES");
        Ok(())
    }

    #[test]
    fn parse_show_tables_with_database() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_tables", "database": "mydb"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES FROM `mydb`");
        Ok(())
    }

    #[test]
    fn parse_show_tables_with_filter() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_tables", "filter": "user%"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES LIKE 'user%'");
        Ok(())
    }

    #[test]
    fn parse_show_tables_with_database_and_filter() -> Result<(), Box<dyn std::error::Error>> {
        let action =
            Action::parse(&json!({"action": "show_tables", "database": "mydb", "filter": "t%"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES FROM `mydb` LIKE 't%'");
        Ok(())
    }

    #[test]
    fn parse_show_stages() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_stages"}))?;
        assert_eq!(action.to_sql(), "SHOW STAGES");
        Ok(())
    }

    #[test]
    fn parse_show_functions_plain() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_functions"}))?;
        assert_eq!(action.to_sql(), "SHOW FUNCTIONS");
        Ok(())
    }

    #[test]
    fn parse_show_functions_with_filter() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_functions", "filter": "to_%"}))?;
        assert_eq!(action.to_sql(), "SHOW FUNCTIONS LIKE 'to_%'");
        Ok(())
    }

    #[test]
    fn parse_missing_action() {
        let err = Action::parse(&json!({"sql": "SELECT 1"})).unwrap_err();
        assert!(err.contains("action"));
    }

    #[test]
    fn parse_unknown_action() {
        let err = Action::parse(&json!({"action": "drop_everything"})).unwrap_err();
        assert!(err.contains("unknown action"));
    }

    #[test]
    fn parse_query_missing_sql() {
        let err = Action::parse(&json!({"action": "query"})).unwrap_err();
        assert!(err.contains("sql"));
    }

    #[test]
    fn parse_exec_missing_sql() {
        let err = Action::parse(&json!({"action": "exec"})).unwrap_err();
        assert!(err.contains("sql"));
    }

    #[test]
    fn parse_describe_missing_table() {
        let err = Action::parse(&json!({"action": "describe"})).unwrap_err();
        assert!(err.contains("table"));
    }

    #[test]
    fn filter_escapes_single_quotes() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_tables", "filter": "it's%"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES LIKE 'it''s%'");
        Ok(())
    }

    #[test]
    fn describe_escapes_backticks() -> Result<(), Box<dyn std::error::Error>> {
        let action =
            Action::parse(&json!({"action": "describe", "table": "us`ers", "database": "my`db"}))?;
        assert_eq!(action.to_sql(), "DESCRIBE `my``db`.`us``ers`");
        Ok(())
    }

    #[test]
    fn show_tables_escapes_database_backticks() -> Result<(), Box<dyn std::error::Error>> {
        let action = Action::parse(&json!({"action": "show_tables", "database": "my`db"}))?;
        assert_eq!(action.to_sql(), "SHOW TABLES FROM `my``db`");
        Ok(())
    }
}

// ── Impact classification ────────────────────────────────────────────────────

fn dummy_tool() -> Result<DatabendTool, Box<dyn std::error::Error>> {
    let pool =
        bendclaw::storage::Pool::new("https://app.databend.com/v1.1", "test-token", "default")?;
    Ok(DatabendTool::new(pool))
}

#[test]
fn impact_read_queries_are_low() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    for sql in &[
        "SELECT * FROM t",
        "SHOW TABLES",
        "DESCRIBE t",
        "EXPLAIN SELECT 1",
    ] {
        let impact = tool.classify_impact(&json!({"action": "query", "sql": sql}));
        assert_eq!(impact, Some(Impact::Low), "expected Low for: {sql}");
    }
    Ok(())
}

#[test]
fn impact_write_queries_are_medium() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    for sql in &[
        "INSERT INTO t VALUES (1)",
        "CREATE TABLE t(id INT)",
        "ALTER TABLE t ADD COLUMN name VARCHAR",
        "COPY INTO t FROM @stage",
    ] {
        let impact = tool.classify_impact(&json!({"action": "exec", "sql": sql}));
        assert_eq!(impact, Some(Impact::Medium), "expected Medium for: {sql}");
    }
    Ok(())
}

#[test]
fn impact_destructive_queries_are_high() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    for sql in &["DROP TABLE t", "TRUNCATE TABLE t"] {
        let impact = tool.classify_impact(&json!({"action": "exec", "sql": sql}));
        assert_eq!(impact, Some(Impact::High), "expected High for: {sql}");
    }
    Ok(())
}

#[test]
fn impact_cte_with_select_is_low() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let sql = "WITH cte AS (SELECT 1) SELECT * FROM cte";
    let impact = tool.classify_impact(&json!({"action": "query", "sql": sql}));
    assert_eq!(impact, Some(Impact::Low));
    Ok(())
}

#[test]
fn impact_cte_with_insert_is_medium() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let sql = "WITH cte AS (SELECT 1) INSERT INTO t SELECT * FROM cte";
    let impact = tool.classify_impact(&json!({"action": "exec", "sql": sql}));
    assert_eq!(impact, Some(Impact::Medium));
    Ok(())
}

#[test]
fn impact_cte_with_drop_is_high() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let sql = "WITH cte AS (SELECT 1) DROP TABLE t";
    let impact = tool.classify_impact(&json!({"action": "exec", "sql": sql}));
    assert_eq!(impact, Some(Impact::High));
    Ok(())
}

#[test]
fn impact_cte_with_unicode_literal_and_drop_is_high() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let sql = "WITH cte AS (SELECT 'ß') DROP TABLE t";
    let impact = tool.classify_impact(&json!({"action": "exec", "sql": sql}));
    assert_eq!(impact, Some(Impact::High));
    Ok(())
}

#[test]
fn impact_cte_with_function_call_select_is_low() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let sql = "WITH cte AS (SELECT 1) SELECT IFNULL(x, 0) FROM cte";
    let impact = tool.classify_impact(&json!({"action": "query", "sql": sql}));
    assert_eq!(impact, Some(Impact::Low));
    Ok(())
}

#[test]
fn impact_convenience_actions_are_low() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    for action in &[
        "show_databases",
        "show_tables",
        "show_stages",
        "show_functions",
        "describe",
    ] {
        let impact = tool.classify_impact(&json!({"action": action}));
        assert_eq!(impact, Some(Impact::Low), "expected Low for: {action}");
    }
    Ok(())
}

// ── Tool trait ───────────────────────────────────────────────────────────────

#[test]
fn tool_name() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    assert_eq!(tool.name(), "databend");
    assert_eq!(tool.name(), ToolId::Databend.as_str());
    Ok(())
}

#[test]
fn tool_op_type() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    assert_eq!(tool.op_type(), OpType::Databend);
    Ok(())
}

#[test]
fn tool_schema_has_required_action() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let schema = tool.parameters_schema();
    let props = schema
        .get("properties")
        .ok_or_else(|| anyhow::anyhow!("missing properties"))?;
    assert!(props.get("action").is_some());
    assert!(props.get("sql").is_some());
    assert!(props.get("table").is_some());
    assert!(props.get("database").is_some());
    assert!(props.get("filter").is_some());
    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .ok_or_else(|| anyhow::anyhow!("missing required array"))?;
    assert_eq!(required.len(), 1);
    assert_eq!(required[0], "action");
    Ok(())
}

#[test]
fn summarize_query() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let summary = tool.summarize(&json!({"action": "query", "sql": "SELECT * FROM users"}));
    assert_eq!(summary, "SELECT * FROM users");
    Ok(())
}

#[test]
fn summarize_describe() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let summary = tool.summarize(&json!({"action": "describe", "table": "users"}));
    assert_eq!(summary, "DESCRIBE `users`");
    Ok(())
}

#[test]
fn summarize_truncates_long_sql() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let long_sql = "SELECT ".to_string() + &"a, ".repeat(100);
    let summary = tool.summarize(&json!({"action": "query", "sql": long_sql}));
    assert!(summary.len() <= 123); // 120 + "..."
    assert!(summary.ends_with("..."));
    Ok(())
}

#[test]
fn summarize_truncates_multibyte_safely() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    // Each char is 3 bytes; ensure we don't panic on a mid-char boundary.
    let long_sql = "SELECT ".to_string() + &"\u{4e16}".repeat(100);
    let summary = tool.summarize(&json!({"action": "query", "sql": long_sql}));
    assert!(summary.ends_with("..."));
    Ok(())
}

#[test]
fn summarize_invalid_action() -> Result<(), Box<dyn std::error::Error>> {
    let tool = dummy_tool()?;
    let summary = tool.summarize(&json!({"action": "bad"}));
    assert_eq!(summary, "databend");
    Ok(())
}
