use std::fmt::Display;
use std::str::FromStr;

use serde::de::DeserializeOwned;

/// Escape a string for safe inclusion in a SQL single-quoted literal.
/// Escapes backslashes first (so JSON escape sequences like \n are preserved),
/// then doubles single quotes.
pub fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}

/// Escape a string for safe use inside a `QUERY('...')` Lucene expression.
/// Escapes Lucene special characters with backslash and strips single quotes
/// (which conflict with the SQL string delimiter and are not meaningful for FTS).
pub fn escape_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for c in s.chars() {
        if c == '\'' {
            continue;
        }
        if matches!(
            c,
            '+' | '-'
                | '&'
                | '|'
                | '!'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '"'
                | '~'
                | '*'
                | '?'
                | ':'
                | '\\'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape a string for safe use in a SQL LIKE pattern.
/// Uses `^` as the ESCAPE character (Databend's tokenizer rejects backslash in ESCAPE clauses).
/// Escapes `%`, `_`, `^`, and `'` so they are treated as literals.
pub fn escape_like(s: &str) -> String {
    s.replace('^', "^^")
        .replace('%', "^%")
        .replace('_', "^_")
        .replace('\'', "''")
}

/// Escape a string for safe inclusion in a backtick-quoted SQL identifier.
pub fn escape_ident(s: &str) -> String {
    s.replace('`', "``")
}

/// Extract a column value as `String` from a JSON array row.
pub fn col(row: &serde_json::Value, idx: usize) -> String {
    row.as_array()
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn col_opt(row: &serde_json::Value, idx: usize) -> Option<String> {
    row.as_array()
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null"))
}

pub fn col_i32(row: &serde_json::Value, idx: usize) -> crate::base::Result<i32> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn col_i64(row: &serde_json::Value, idx: usize) -> crate::base::Result<i64> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn col_u32(row: &serde_json::Value, idx: usize) -> crate::base::Result<u32> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn col_u64(row: &serde_json::Value, idx: usize) -> crate::base::Result<u64> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn col_f32(row: &serde_json::Value, idx: usize) -> crate::base::Result<f32> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn col_f64(row: &serde_json::Value, idx: usize) -> crate::base::Result<f64> {
    let raw = col(row, idx);
    parse_number(&raw, format!("col {idx}"))
}

pub fn parse_number<T, E>(raw: &str, label: impl Into<String>) -> crate::base::Result<T>
where
    T: FromStr<Err = E>,
    E: Display,
{
    let label = label.into();
    raw.parse().map_err(|e| {
        crate::base::ErrorCode::storage_serde(format!("{label}: {e} (value: '{raw}')"))
    })
}

pub fn parse_json<T: DeserializeOwned>(
    raw: &str,
    label: impl Into<String>,
) -> crate::base::Result<T> {
    let label = label.into();
    serde_json::from_str(raw).map_err(|e| {
        crate::base::ErrorCode::storage_serde(format!("{label}: {e} (value: '{raw}')"))
    })
}

pub fn agg_u64_or_zero(row: Option<&serde_json::Value>, idx: usize) -> crate::base::Result<u64> {
    parse_aggregate_or_zero(row, idx)
}

pub fn agg_i64_or_zero(row: Option<&serde_json::Value>, idx: usize) -> crate::base::Result<i64> {
    parse_aggregate_or_zero(row, idx)
}

pub fn agg_f64_or_zero(row: Option<&serde_json::Value>, idx: usize) -> crate::base::Result<f64> {
    parse_aggregate_or_zero(row, idx)
}

pub fn agg_str(row: Option<&serde_json::Value>, idx: usize) -> String {
    row_str(row, idx).unwrap_or("").to_string()
}

fn parse_aggregate_or_zero<T, E>(
    row: Option<&serde_json::Value>,
    idx: usize,
) -> crate::base::Result<T>
where
    T: FromStr<Err = E> + Default,
    E: Display,
{
    match row_str(row, idx) {
        Some(raw) if !raw.is_empty() && !raw.eq_ignore_ascii_case("null") => {
            parse_number(raw, format!("col {idx}"))
        }
        _ => Ok(T::default()),
    }
}

fn row_str(row: Option<&serde_json::Value>, idx: usize) -> Option<&str> {
    row.and_then(|r| r.as_array())
        .and_then(|a| a.get(idx))
        .and_then(|v| v.as_str())
}

/// Typed SQL value — handles escaping and quoting automatically.
pub enum SqlVal<'a> {
    Str(&'a str),
    Int(i64),
    Float(f64),
    /// Raw SQL expression, rendered verbatim (e.g. `"NOW()"`).
    Raw(&'a str),
    Null,
}

impl<'a> SqlVal<'a> {
    pub fn render(&self) -> String {
        match self {
            SqlVal::Str(s) => format!("'{}'", escape(s)),
            SqlVal::Int(n) => n.to_string(),
            SqlVal::Float(f) => f.to_string(),
            SqlVal::Raw(expr) => expr.to_string(),
            SqlVal::Null => "NULL".to_string(),
        }
    }

    pub fn str_or_null(opt: Option<&'a str>) -> Self {
        match opt {
            Some(s) => SqlVal::Str(s),
            None => SqlVal::Null,
        }
    }
}

impl<'a> From<&'a str> for SqlVal<'a> {
    fn from(s: &'a str) -> Self {
        SqlVal::Str(s)
    }
}

impl<'a> From<&'a String> for SqlVal<'a> {
    fn from(s: &'a String) -> Self {
        SqlVal::Str(s.as_str())
    }
}

impl<'a> From<i64> for SqlVal<'a> {
    fn from(n: i64) -> Self {
        SqlVal::Int(n)
    }
}

impl<'a> From<i32> for SqlVal<'a> {
    fn from(n: i32) -> Self {
        SqlVal::Int(n as i64)
    }
}

impl<'a> From<f64> for SqlVal<'a> {
    fn from(f: f64) -> Self {
        SqlVal::Float(f)
    }
}

impl<'a> From<u64> for SqlVal<'a> {
    fn from(n: u64) -> Self {
        SqlVal::Int(n as i64)
    }
}

pub struct Sql;

impl Sql {
    pub fn insert(table: &str) -> InsertBuilder {
        InsertBuilder {
            table: table.to_string(),
            replace: false,
            columns: Vec::new(),
            values: Vec::new(),
            conflict_cols: None,
        }
    }

    /// Start building a REPLACE INTO statement.
    /// Call `.on_conflict("col1, col2")` to specify the PRIMARY KEY columns.
    pub fn replace(table: &str) -> InsertBuilder {
        InsertBuilder {
            table: table.to_string(),
            replace: true,
            columns: Vec::new(),
            values: Vec::new(),
            conflict_cols: None,
        }
    }

    /// Start building a SELECT statement.
    pub fn select(columns: &str) -> SelectBuilder {
        SelectBuilder {
            columns: columns.to_string(),
            table: String::new(),
            wheres: Vec::new(),
            group: None,
            order: None,
            limit: None,
        }
    }

    pub fn update(table: &str) -> UpdateBuilder {
        UpdateBuilder {
            table: table.to_string(),
            sets: Vec::new(),
            wheres: Vec::new(),
        }
    }

    /// Start building a DELETE statement.
    pub fn delete(table: &str) -> DeleteBuilder {
        DeleteBuilder {
            table: table.to_string(),
            wheres: Vec::new(),
        }
    }

    /// Start building a batch INSERT statement with multiple rows.
    pub fn insert_batch(table: &str, columns: &[&str]) -> BatchInsertBuilder {
        BatchInsertBuilder {
            table: table.to_string(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            rows: Vec::new(),
        }
    }
}

#[must_use]
pub struct InsertBuilder {
    table: String,
    replace: bool,
    columns: Vec<String>,
    values: Vec<String>,
    /// Conflict columns for REPLACE INTO … ON (cols) VALUES (…) — required by Databend.
    conflict_cols: Option<String>,
}

impl InsertBuilder {
    pub fn value<'a>(mut self, column: &str, val: impl Into<SqlVal<'a>>) -> Self {
        self.columns.push(column.to_string());
        self.values.push(val.into().render());
        self
    }

    /// Set the conflict-key columns for `REPLACE INTO … ON (cols) VALUES (…)`.
    /// Required for Databend; corresponds to the table's PRIMARY KEY.
    pub fn on_conflict(mut self, cols: &str) -> Self {
        self.conflict_cols = Some(cols.to_string());
        self
    }

    pub fn build(self) -> String {
        let cols = self.columns.join(", ");
        let vals = self.values.join(", ");
        if self.replace {
            let on = self.conflict_cols.as_deref().unwrap_or(&cols); // fall back to all columns if not specified
            format!(
                "REPLACE INTO {} ({}) ON ({}) VALUES ({})",
                self.table, cols, on, vals
            )
        } else {
            format!("INSERT INTO {} ({}) VALUES ({})", self.table, cols, vals)
        }
    }
}

#[must_use]
pub struct SelectBuilder {
    columns: String,
    table: String,
    wheres: Vec<String>,
    group: Option<String>,
    order: Option<String>,
    limit: Option<u64>,
}

impl SelectBuilder {
    pub fn from(mut self, table: &str) -> Self {
        self.table = table.to_string();
        self
    }

    pub fn where_eq<'a>(mut self, column: &str, val: impl Into<SqlVal<'a>>) -> Self {
        self.wheres
            .push(format!("{} = {}", column, val.into().render()));
        self
    }

    pub fn where_raw(mut self, condition: &str) -> Self {
        self.wheres.push(condition.to_string());
        self
    }

    pub fn group_by(mut self, clause: &str) -> Self {
        self.group = Some(clause.to_string());
        self
    }

    pub fn order_by(mut self, clause: &str) -> Self {
        self.order = Some(clause.to_string());
        self
    }

    pub fn limit(mut self, n: u64) -> Self {
        self.limit = Some(n);
        self
    }

    pub fn build(self) -> String {
        let mut sql = format!("SELECT {} FROM {}", self.columns, self.table);
        if !self.wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.wheres.join(" AND "));
        }
        if let Some(group) = self.group {
            sql.push_str(" GROUP BY ");
            sql.push_str(&group);
        }
        if let Some(order) = self.order {
            sql.push_str(" ORDER BY ");
            sql.push_str(&order);
        }
        if let Some(limit) = self.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }
        sql
    }
}

#[must_use]
pub struct UpdateBuilder {
    table: String,
    sets: Vec<String>,
    wheres: Vec<String>,
}

impl UpdateBuilder {
    pub fn set<'a>(mut self, column: &str, val: impl Into<SqlVal<'a>>) -> Self {
        self.sets
            .push(format!("{} = {}", column, val.into().render()));
        self
    }

    /// Set a column only if the value is `Some`. Skips `None`.
    pub fn set_opt(self, column: &str, val: Option<&str>) -> Self {
        match val {
            Some(v) => self.set(column, v),
            None => self,
        }
    }

    pub fn set_raw(mut self, column: &str, expr: &str) -> Self {
        self.sets.push(format!("{column} = {expr}"));
        self
    }

    pub fn where_eq<'a>(mut self, column: &str, val: impl Into<SqlVal<'a>>) -> Self {
        self.wheres
            .push(format!("{} = {}", column, val.into().render()));
        self
    }

    pub fn where_raw(mut self, condition: &str) -> Self {
        self.wheres.push(condition.to_string());
        self
    }

    pub fn has_sets(&self) -> bool {
        !self.sets.is_empty()
    }

    pub fn build(self) -> String {
        let mut sql = format!("UPDATE {} SET {}", self.table, self.sets.join(", "));
        if !self.wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.wheres.join(" AND "));
        }
        sql
    }
}

#[must_use]
pub struct DeleteBuilder {
    table: String,
    wheres: Vec<String>,
}

impl DeleteBuilder {
    pub fn where_eq<'a>(mut self, column: &str, val: impl Into<SqlVal<'a>>) -> Self {
        self.wheres
            .push(format!("{} = {}", column, val.into().render()));
        self
    }

    pub fn where_raw(mut self, expr: &str) -> Self {
        self.wheres.push(expr.to_string());
        self
    }

    pub fn build(self) -> String {
        let mut sql = format!("DELETE FROM {}", self.table);
        if !self.wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&self.wheres.join(" AND "));
        }
        sql
    }
}

/// Builds a multi-row INSERT statement: `INSERT INTO t (c1, c2) VALUES (...), (...)`.
#[must_use]
pub struct BatchInsertBuilder {
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl BatchInsertBuilder {
    pub fn row<'a>(mut self, values: &[SqlVal<'a>]) -> Self {
        self.rows.push(values.iter().map(|v| v.render()).collect());
        self
    }

    pub fn build(self) -> Option<String> {
        if self.rows.is_empty() {
            return None;
        }
        let cols = self.columns.join(", ");
        let rows: Vec<String> = self
            .rows
            .iter()
            .map(|r| format!("({})", r.join(", ")))
            .collect();
        Some(format!(
            "INSERT INTO {} ({}) VALUES {}",
            self.table,
            cols,
            rows.join(", ")
        ))
    }
}
