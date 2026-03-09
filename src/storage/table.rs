//! Generic table access layer for Databend.
//!
//! Eliminates repetitive SQL building and row parsing across stores.
//! Each domain store keeps its own table schema but delegates SQL
//! mechanics to [`DatabendTable`].

use super::pool::Pool;
use super::sql;
use super::sql::Sql;
use super::sql::SqlVal;
use crate::base::Result;

/// Maps a raw JSON row to a domain type.
pub trait RowMapper: Send + Sync {
    type Entity;
    /// Column expression for SELECT (e.g. "id, name, TO_VARCHAR(created_at)").
    fn columns(&self) -> &str;
    /// Parse a single row into the domain entity.
    fn parse(&self, row: &serde_json::Value) -> Self::Entity;
}

/// A WHERE condition: column name + rendered SQL value.
pub struct Where<'a>(pub &'a str, pub SqlVal<'a>);

/// Generic table operations backed by Databend.
#[derive(Clone)]
pub struct DatabendTable<M> {
    pool: Pool,
    table: String,
    mapper: M,
}

impl<M: RowMapper> DatabendTable<M> {
    pub fn new(pool: Pool, table: &str, mapper: M) -> Self {
        Self {
            pool,
            table: table.to_string(),
            mapper,
        }
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    fn apply_wheres(q: sql::SelectBuilder, wheres: &[Where<'_>]) -> sql::SelectBuilder {
        let mut q = q;
        for w in wheres {
            q = q.where_raw(&format!("{} = {}", w.0, w.1.render()));
        }
        q
    }

    fn apply_delete_wheres(q: sql::DeleteBuilder, wheres: &[Where<'_>]) -> sql::DeleteBuilder {
        let mut q = q;
        for w in wheres {
            q = q.where_raw(&format!("{} = {}", w.0, w.1.render()));
        }
        q
    }

    // ── Single-row reads ──

    pub async fn get(&self, wheres: &[Where<'_>]) -> Result<Option<M::Entity>> {
        let q = Self::apply_wheres(Sql::select(self.mapper.columns()).from(&self.table), wheres);
        let query = q.limit(1).build();
        let row = self.pool.query_row(&query).await?;
        Ok(row.as_ref().map(|r| self.mapper.parse(r)))
    }

    pub async fn get_where(&self, condition: &str) -> Result<Option<M::Entity>> {
        let query = Sql::select(self.mapper.columns())
            .from(&self.table)
            .where_raw(condition)
            .limit(1)
            .build();
        let row = self.pool.query_row(&query).await?;
        Ok(row.as_ref().map(|r| self.mapper.parse(r)))
    }

    // ── Multi-row reads ──

    pub async fn list(
        &self,
        wheres: &[Where<'_>],
        order: &str,
        limit: u64,
    ) -> Result<Vec<M::Entity>> {
        let q = Self::apply_wheres(Sql::select(self.mapper.columns()).from(&self.table), wheres);
        let query = q.order_by(order).limit(limit).build();
        let rows = self.pool.query_all(&query).await?;
        Ok(rows.iter().map(|r| self.mapper.parse(r)).collect())
    }

    pub async fn list_where(
        &self,
        condition: &str,
        order: &str,
        limit: u64,
    ) -> Result<Vec<M::Entity>> {
        let query = Sql::select(self.mapper.columns())
            .from(&self.table)
            .where_raw(condition)
            .order_by(order)
            .limit(limit)
            .build();
        let rows = self.pool.query_all(&query).await?;
        Ok(rows.iter().map(|r| self.mapper.parse(r)).collect())
    }

    // ── Writes ──

    pub async fn insert(&self, values: &[(&str, SqlVal<'_>)]) -> Result<()> {
        let mut builder = Sql::insert(&self.table);
        for (col, val) in values {
            builder = builder.value(col, SqlVal::Raw(&val.render()));
        }
        self.pool.exec(&builder.build()).await
    }

    pub async fn upsert(&self, values: &[(&str, SqlVal<'_>)], conflict_key: &str) -> Result<()> {
        let mut builder = Sql::replace(&self.table).on_conflict(conflict_key);
        for (col, val) in values {
            builder = builder.value(col, SqlVal::Raw(&val.render()));
        }
        self.pool.exec(&builder.build()).await
    }

    pub async fn insert_batch(&self, columns: &[&str], rows: &[Vec<SqlVal<'_>>]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut batch = Sql::insert_batch(&self.table, columns);
        for row in rows {
            batch = batch.row(row);
        }
        if let Some(query) = batch.build() {
            self.pool.exec(&query).await?;
        }
        Ok(())
    }

    // ── Deletes ──

    pub async fn delete(&self, wheres: &[Where<'_>]) -> Result<()> {
        let q = Self::apply_delete_wheres(Sql::delete(&self.table), wheres);
        self.pool.exec(&q.build()).await
    }

    pub async fn delete_where(&self, condition: &str) -> Result<()> {
        let q = Sql::delete(&self.table).where_raw(condition);
        self.pool.exec(&q.build()).await
    }

    // ── Search ──

    pub async fn search_fts(
        &self,
        fts_column: &str,
        query: &str,
        extra_where: Option<&str>,
        order: &str,
        limit: u64,
    ) -> Result<Vec<(M::Entity, f32)>> {
        self.search_fts_in_scope(fts_column, query, extra_where, None, order, limit)
            .await
    }

    pub async fn search_fts_in_scope(
        &self,
        fts_column: &str,
        query: &str,
        extra_where: Option<&str>,
        scope_where: Option<&[Where<'_>]>,
        order: &str,
        limit: u64,
    ) -> Result<Vec<(M::Entity, f32)>> {
        let score_cols = format!("{}, SCORE()", self.mapper.columns());
        let mut q = Sql::select(&score_cols)
            .from(&self.table)
            .where_raw(&format!("MATCH({}, '{}')", fts_column, sql::escape(query)));
        if let Some(wheres) = scope_where {
            q = Self::apply_wheres(q, wheres);
        }
        if let Some(cond) = extra_where {
            q = q.where_raw(cond);
        }
        let sql_str = q.order_by(order).limit(limit).build();
        let rows = self.pool.query_all(&sql_str).await?;
        Ok(rows
            .iter()
            .map(|r| {
                let entity = self.mapper.parse(r);
                let num_cols = self.mapper.columns().split(',').count();
                let score: f32 = sql::col(r, num_cols).parse().unwrap_or(0.0);
                (entity, score)
            })
            .collect())
    }

    // ── Aggregation ──

    pub async fn aggregate(
        &self,
        select_expr: &str,
        condition: Option<&str>,
    ) -> Result<Option<serde_json::Value>> {
        let mut q = Sql::select(select_expr).from(&self.table);
        if let Some(cond) = condition {
            q = q.where_raw(cond);
        }
        self.pool.query_row(&q.build()).await
    }
}
