use anyhow::Context as _;
use anyhow::Result;
use bendclaw::storage::sql::*;
use serde_json::json;

// ── escape tests ─────────────────────────────────────

#[test]
fn test_escape_no_quotes() -> Result<()> {
    assert_eq!(escape("hello"), "hello");
    Ok(())
}

#[test]
fn test_escape_single_quotes() -> Result<()> {
    assert_eq!(escape("it's"), "it''s");
    Ok(())
}

#[test]
fn test_escape_multiple_quotes() -> Result<()> {
    assert_eq!(escape("a'b'c"), "a''b''c");
    Ok(())
}

#[test]
fn test_escape_empty() -> Result<()> {
    assert_eq!(escape(""), "");
    Ok(())
}

// ── col tests ────────────────────────────────────────

#[test]
fn test_col_valid_row() -> Result<()> {
    let row = json!(["alice", "42", "hello"]);
    assert_eq!(col(&row, 0), "alice");
    assert_eq!(col(&row, 1), "42");
    assert_eq!(col(&row, 2), "hello");
    Ok(())
}

#[test]
fn test_col_out_of_bounds() -> Result<()> {
    let row = json!(["only"]);
    assert_eq!(col(&row, 5), "");
    Ok(())
}

#[test]
fn test_col_not_array() -> Result<()> {
    let row = json!({"key": "value"});
    assert_eq!(col(&row, 0), "");
    Ok(())
}

#[test]
fn test_col_opt_present() -> Result<()> {
    let row = json!(["hello", ""]);
    assert_eq!(col_opt(&row, 0), Some("hello".to_string()));
    Ok(())
}

#[test]
fn test_col_opt_empty_returns_none() -> Result<()> {
    let row = json!(["hello", ""]);
    assert_eq!(col_opt(&row, 1), None);
    Ok(())
}

#[test]
fn test_col_opt_missing_returns_none() -> Result<()> {
    let row = json!(["hello"]);
    assert_eq!(col_opt(&row, 5), None);
    Ok(())
}

#[test]
fn test_col_i32_valid() -> Result<()> {
    let row = json!(["123"]);
    assert_eq!(col_i32(&row, 0)?, 123);
    Ok(())
}

#[test]
fn test_col_i32_invalid() -> Result<()> {
    let row = json!(["abc"]);
    assert!(col_i32(&row, 0).is_err());
    Ok(())
}

#[test]
fn test_col_i64_valid() -> Result<()> {
    let row = json!(["9999999999"]);
    assert_eq!(col_i64(&row, 0)?, 9_999_999_999);
    Ok(())
}

#[test]
fn test_col_i64_missing() -> Result<()> {
    let row = json!([]);
    assert!(col_i64(&row, 0).is_err());
    Ok(())
}

// ── SqlVal tests ─────────────────────────────────────

#[test]
fn test_sql_val_str_escaping() -> Result<()> {
    assert_eq!(SqlVal::Str("hello").render(), "'hello'");
    assert_eq!(SqlVal::Str("it's").render(), "'it''s'");
    Ok(())
}

#[test]
fn test_sql_val_int() -> Result<()> {
    assert_eq!(SqlVal::Int(42).render(), "42");
    assert_eq!(SqlVal::Int(-1).render(), "-1");
    Ok(())
}

#[test]
fn test_sql_val_float() -> Result<()> {
    assert_eq!(SqlVal::Float(3.14).render(), "3.14");
    Ok(())
}

#[test]
fn test_sql_val_raw() -> Result<()> {
    assert_eq!(SqlVal::Raw("version + 1").render(), "version + 1");
    assert_eq!(SqlVal::Raw("NOW()").render(), "NOW()");
    Ok(())
}

#[test]
fn test_sql_val_null() -> Result<()> {
    assert_eq!(SqlVal::Null.render(), "NULL");
    Ok(())
}

#[test]
fn test_sql_val_str_or_null() -> Result<()> {
    assert_eq!(SqlVal::str_or_null(Some("hi")).render(), "'hi'");
    assert_eq!(SqlVal::str_or_null(None).render(), "NULL");
    Ok(())
}

#[test]
fn test_sql_val_from_impls() -> Result<()> {
    let val: SqlVal = "hello".into();
    assert_eq!(val.render(), "'hello'");

    let s = String::from("world");
    let val: SqlVal = (&s).into();
    assert_eq!(val.render(), "'world'");

    let val: SqlVal = 42i64.into();
    assert_eq!(val.render(), "42");

    let val: SqlVal = 7i32.into();
    assert_eq!(val.render(), "7");

    let val: SqlVal = 100u64.into();
    assert_eq!(val.render(), "100");

    let val: SqlVal = 2.5f64.into();
    assert_eq!(val.render(), "2.5");
    Ok(())
}

// ── BatchInsertBuilder tests ──

#[test]
fn test_batch_insert_builder() -> Result<()> {
    let sql = Sql::insert_batch("logs", &["id", "msg"])
        .row(&[SqlVal::Str("1"), SqlVal::Str("hello")])
        .row(&[SqlVal::Str("2"), SqlVal::Str("world")])
        .build();
    let s = sql.context("expected Some sql")?;
    assert!(s.contains("('1', 'hello'), ('2', 'world')"));
    Ok(())
}

#[test]
fn test_batch_insert_builder_empty() -> Result<()> {
    let sql = Sql::insert_batch("logs", &["id"]).build();
    assert!(sql.is_none());
    Ok(())
}

// ── ReplaceBuilder tests ─────────────────────────────

#[test]
fn test_replace_builder_with_on_conflict() -> Result<()> {
    let sql = Sql::replace("session_memory")
        .value("tenant_id", "t1")
        .value("user_id", "u1")
        .value("session_id", "s1")
        .value("key", "lang")
        .value("value", "rust")
        .on_conflict("tenant_id, user_id, session_id, key")
        .build();
    assert_eq!(
        sql,
        "REPLACE INTO session_memory (tenant_id, user_id, session_id, key, value) \
         ON (tenant_id, user_id, session_id, key) \
         VALUES ('t1', 'u1', 's1', 'lang', 'rust')"
    );
    Ok(())
}

#[test]
fn test_replace_builder_without_on_conflict_falls_back_to_all_cols() -> Result<()> {
    let sql = Sql::replace("t").value("a", "x").value("b", "y").build();
    assert_eq!(sql, "REPLACE INTO t (a, b) ON (a, b) VALUES ('x', 'y')");
    Ok(())
}

#[test]
fn test_replace_builder_with_null_value() -> Result<()> {
    let sql = Sql::replace("skills")
        .value("tenant_id", "t1")
        .value("user_id", SqlVal::Null)
        .value("name", "grep")
        .on_conflict("tenant_id, name")
        .build();
    assert_eq!(
        sql,
        "REPLACE INTO skills (tenant_id, user_id, name) \
         ON (tenant_id, name) \
         VALUES ('t1', NULL, 'grep')"
    );
    Ok(())
}

// ── InsertBuilder tests ──────────────────────────────

#[test]
fn test_insert_builder() -> Result<()> {
    let sql = Sql::insert("users")
        .value("id", "abc-123")
        .value("name", "O'Brien")
        .value("age", 30i64)
        .value("score", 9.5f64)
        .build();
    assert_eq!(
        sql,
        "INSERT INTO users (id, name, age, score) VALUES ('abc-123', 'O''Brien', 30, 9.5)"
    );
    Ok(())
}

#[test]
fn test_insert_builder_with_null() -> Result<()> {
    let sql = Sql::insert("t")
        .value("a", "x")
        .value("b", SqlVal::Null)
        .build();
    assert_eq!(sql, "INSERT INTO t (a, b) VALUES ('x', NULL)");
    Ok(())
}

// ── SelectBuilder tests ──────────────────────────────

#[test]
fn test_select_builder_simple() -> Result<()> {
    let sql = Sql::select("id, name")
        .from("users")
        .where_eq("id", "abc")
        .build();
    assert_eq!(sql, "SELECT id, name FROM users WHERE id = 'abc'");
    Ok(())
}

#[test]
fn test_select_builder_full() -> Result<()> {
    let sql = Sql::select("*")
        .from("plans")
        .where_eq("user_id", "u1")
        .where_eq("tenant_id", "tenant-1")
        .order_by("created_at DESC")
        .limit(100)
        .build();
    assert_eq!(
        sql,
        "SELECT * FROM plans WHERE user_id = 'u1' AND tenant_id = 'tenant-1' ORDER BY created_at DESC LIMIT 100"
    );
    Ok(())
}

#[test]
fn test_select_builder_no_where() -> Result<()> {
    let sql = Sql::select("COUNT(*)").from("t").build();
    assert_eq!(sql, "SELECT COUNT(*) FROM t");
    Ok(())
}

#[test]
fn test_select_builder_where_eq_escapes_quotes() -> Result<()> {
    let sql = Sql::select("*")
        .from("users")
        .where_eq("name", "O'Brien")
        .build();
    assert_eq!(sql, "SELECT * FROM users WHERE name = 'O''Brien'");
    Ok(())
}

#[test]
fn test_select_builder_where_raw() -> Result<()> {
    let sql = Sql::select("*")
        .from("t")
        .where_raw("action != 'delete'")
        .build();
    assert_eq!(sql, "SELECT * FROM t WHERE action != 'delete'");
    Ok(())
}

// ── UpdateBuilder tests ──────────────────────────────

#[test]
fn test_update_builder() -> Result<()> {
    let sql = Sql::update("plans")
        .set("status", "active")
        .set_raw("version", "version + 1")
        .where_eq("id", "p1")
        .build();
    assert_eq!(
        sql,
        "UPDATE plans SET status = 'active', version = version + 1 WHERE id = 'p1'"
    );
    Ok(())
}

#[test]
fn test_update_builder_set_opt() -> Result<()> {
    let title: Option<&str> = Some("new title");
    let desc: Option<&str> = None;
    let mut b = Sql::update("t")
        .set_opt("title", title)
        .set_opt("desc", desc);
    assert!(b.has_sets());
    b = b.where_eq("id", "x");
    let sql = b.build();
    assert_eq!(sql, "UPDATE t SET title = 'new title' WHERE id = 'x'");
    Ok(())
}

#[test]
fn test_update_builder_has_sets_empty() -> Result<()> {
    let b = Sql::update("t").set_opt("a", None).set_opt("b", None);
    assert!(!b.has_sets());
    Ok(())
}

#[test]
fn test_update_builder_where_eq_escapes_quotes() -> Result<()> {
    let sql = Sql::update("users")
        .set("status", "active")
        .where_eq("name", "O'Brien")
        .build();
    assert_eq!(
        sql,
        "UPDATE users SET status = 'active' WHERE name = 'O''Brien'"
    );
    Ok(())
}

// ── DeleteBuilder tests ──────────────────────────────

#[test]
fn test_delete_builder() -> Result<()> {
    let sql = Sql::delete("steps").where_eq("plan_id", "p1").build();
    assert_eq!(sql, "DELETE FROM steps WHERE plan_id = 'p1'");
    Ok(())
}

#[test]
fn test_delete_builder_no_where() -> Result<()> {
    let sql = Sql::delete("temp").build();
    assert_eq!(sql, "DELETE FROM temp");
    Ok(())
}

#[test]
fn test_delete_builder_where_eq_escapes_quotes() -> Result<()> {
    let sql = Sql::delete("users").where_eq("name", "O'Brien").build();
    assert_eq!(sql, "DELETE FROM users WHERE name = 'O''Brien'");
    Ok(())
}

// ── escape_like tests ───────────────────────────────

#[test]
fn escape_like_no_special_chars() {
    assert_eq!(escape_like("hello"), "hello");
}

#[test]
fn escape_like_percent() {
    assert_eq!(escape_like("100%"), "100^%");
}

#[test]
fn escape_like_underscore() {
    assert_eq!(escape_like("a_b"), "a^_b");
}

#[test]
fn escape_like_backslash() {
    // Backslash is no longer special for LIKE escape; passes through unchanged
    assert_eq!(escape_like("a\\b"), "a\\b");
}

#[test]
fn escape_like_single_quote() {
    assert_eq!(escape_like("it's"), "it''s");
}

#[test]
fn escape_like_all_special() {
    assert_eq!(escape_like("%_^'"), "^%^_^^''");
}

#[test]
fn escape_like_caret() {
    assert_eq!(escape_like("a^b"), "a^^b");
}

// ── escape_ident tests ──────────────────────────────

#[test]
fn escape_ident_no_backtick() {
    assert_eq!(escape_ident("stage"), "stage");
}

#[test]
fn escape_ident_with_backtick() {
    assert_eq!(escape_ident("my`col"), "my``col");
}

#[test]
fn escape_ident_empty() {
    assert_eq!(escape_ident(""), "");
}

// ── col_opt null handling ───────────────────────────

#[test]
fn col_opt_null_string_returns_none() {
    let row = json!(["NULL"]);
    assert_eq!(col_opt(&row, 0), None);
}

#[test]
fn col_opt_null_lowercase_returns_none() {
    let row = json!(["null"]);
    assert_eq!(col_opt(&row, 0), None);
}

// ── SelectBuilder group_by ──────────────────────────

#[test]
fn select_builder_group_by() {
    let sql = Sql::select("status, COUNT(*)")
        .from("runs")
        .group_by("status")
        .build();
    assert_eq!(sql, "SELECT status, COUNT(*) FROM runs GROUP BY status");
}

#[test]
fn select_builder_group_by_with_order_and_limit() {
    let sql = Sql::select("status, COUNT(*) as cnt")
        .from("runs")
        .where_raw("agent_id = 'a1'")
        .group_by("status")
        .order_by("cnt DESC")
        .limit(10)
        .build();
    assert_eq!(
        sql,
        "SELECT status, COUNT(*) as cnt FROM runs WHERE agent_id = 'a1' GROUP BY status ORDER BY cnt DESC LIMIT 10"
    );
}

// ── DeleteBuilder where_raw ─────────────────────────

#[test]
fn delete_builder_where_raw() {
    let sql = Sql::delete("logs")
        .where_raw("created_at < '2025-01-01'")
        .build();
    assert_eq!(sql, "DELETE FROM logs WHERE created_at < '2025-01-01'");
}

#[test]
fn delete_builder_where_raw_combined() {
    let sql = Sql::delete("logs")
        .where_eq("agent_id", "a1")
        .where_raw("created_at < '2025-01-01'")
        .build();
    assert_eq!(
        sql,
        "DELETE FROM logs WHERE agent_id = 'a1' AND created_at < '2025-01-01'"
    );
}

// ── escape_query tests ──────────────────────────────

#[test]
fn escape_query_plain_text() {
    assert_eq!(escape_query("hello world"), "hello world");
}

#[test]
fn escape_query_special_chars() {
    assert_eq!(escape_query("c++"), "c\\+\\+");
    assert_eq!(escape_query("file:path"), "file\\:path");
    assert_eq!(escape_query("a*b?c"), "a\\*b\\?c");
    assert_eq!(escape_query("(a OR b)"), "\\(a OR b\\)");
    assert_eq!(escape_query("[1 TO 5]"), "\\[1 TO 5\\]");
    assert_eq!(escape_query("{a}"), "\\{a\\}");
    assert_eq!(escape_query("a^2"), "a\\^2");
    assert_eq!(escape_query("~fuzzy"), "\\~fuzzy");
    assert_eq!(escape_query("a&b|c!d"), "a\\&b\\|c\\!d");
    assert_eq!(escape_query("a-b"), "a\\-b");
}

#[test]
fn escape_query_quotes() {
    assert_eq!(escape_query("it's"), "it\\''s");
    assert_eq!(escape_query(r#"say "hi""#), r#"say \"hi\""#);
}

#[test]
fn escape_query_backslash() {
    assert_eq!(escape_query(r"a\b"), r"a\\b");
}

#[test]
fn escape_query_empty() {
    assert_eq!(escape_query(""), "");
}

#[test]
fn escape_query_combined() {
    assert_eq!(
        escape_query("bendclaw:readme (v2)"),
        "bendclaw\\:readme \\(v2\\)"
    );
}
