//! Variable store — IO layer.
//!
//! Pure CRUD over `evotai_meta.variables`. No business logic.

use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;

use crate::base::Result;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;

const TABLE: &str = "evotai_meta.variables";
const MAX_LIST_LIMIT: u32 = 10_000;

// ── Types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VariableScope {
    Private,
    Shared,
}

impl std::fmt::Display for VariableScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Private => write!(f, "private"),
            Self::Shared => write!(f, "shared"),
        }
    }
}

pub fn parse_scope(s: &str) -> VariableScope {
    match s {
        "private" => VariableScope::Private,
        _ => VariableScope::Shared,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    pub id: String,
    pub key: String,
    pub value: String,
    pub secret: bool,
    pub revoked: bool,
    pub user_id: String,
    pub scope: VariableScope,
    pub created_by: String,
    pub last_used_at: Option<String>,
    pub last_used_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ── Trait ──

#[async_trait]
pub trait VariableStore: Send + Sync {
    async fn insert(&self, variable: &Variable) -> Result<()>;
    async fn get(&self, user_id: &str, id: &str) -> Result<Option<Variable>>;
    async fn list_active(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>>;
    async fn list_all(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>>;
    async fn list_shared(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>>;
    async fn update(
        &self,
        user_id: &str,
        id: &str,
        key: &str,
        value: &str,
        secret: bool,
        revoked: bool,
    ) -> Result<()>;
    async fn delete(&self, user_id: &str, id: &str) -> Result<()>;
    async fn touch_last_used(&self, id: &str, agent_id: &str) -> Result<()>;
    async fn touch_last_used_many(&self, ids: &[String], agent_id: &str) -> Result<()>;
}

// ── Implementation ──

pub struct SharedVariableStore {
    pool: Pool,
}

impl SharedVariableStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "id, key, value, secret, revoked, user_id, scope, created_by, \
                    TO_VARCHAR(last_used_at) AS last_used_at, last_used_by, \
                    TO_VARCHAR(created_at) AS created_at, TO_VARCHAR(updated_at) AS updated_at";

fn parse_variable(row: &serde_json::Value) -> Result<Variable> {
    let secret_str: String = sql::col(row, 3);
    let secret = matches!(secret_str.as_str(), "1" | "true");
    let revoked_str: String = sql::col(row, 4);
    let revoked = matches!(revoked_str.as_str(), "1" | "true");
    let last_used_at: String = sql::col(row, 8);
    let last_used_by: String = sql::col(row, 9);
    Ok(Variable {
        id: sql::col(row, 0),
        key: sql::col(row, 1),
        value: sql::col(row, 2),
        secret,
        revoked,
        user_id: sql::col(row, 5),
        scope: parse_scope(&sql::col(row, 6)),
        created_by: sql::col(row, 7),
        last_used_at: if last_used_at.is_empty() {
            None
        } else {
            Some(last_used_at)
        },
        last_used_by: if last_used_by.is_empty() {
            None
        } else {
            Some(last_used_by)
        },
        created_at: sql::col(row, 10),
        updated_at: sql::col(row, 11),
    })
}

#[async_trait]
impl VariableStore for SharedVariableStore {
    async fn insert(&self, v: &Variable) -> Result<()> {
        let scope_str = v.scope.to_string();
        let secret_val = if v.secret { "true" } else { "false" };
        let revoked_val = if v.revoked { "true" } else { "false" };
        let stmt = format!(
            "INSERT INTO {TABLE} (id, key, value, secret, revoked, user_id, scope, created_by, created_at, updated_at) \
             VALUES ({}, {}, {}, {}, {}, {}, {}, {}, NOW(), NOW())",
            SqlVal::Str(&v.id).render(),
            SqlVal::Str(&v.key).render(),
            SqlVal::Str(&v.value).render(),
            secret_val,
            revoked_val,
            SqlVal::Str(&v.user_id).render(),
            SqlVal::Str(&scope_str).render(),
            SqlVal::Str(&v.created_by).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn get(&self, user_id: &str, id: &str) -> Result<Option<Variable>> {
        let stmt = format!(
            "SELECT {COLS} FROM {TABLE} WHERE user_id = {} AND id = {} LIMIT 1",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(id).render(),
        );
        let row = self.pool.query_row(&stmt).await?;
        row.as_ref().map(parse_variable).transpose()
    }

    async fn list_active(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>> {
        let lim = limit.min(MAX_LIST_LIMIT);
        let uid = SqlVal::Str(user_id).render();
        let stmt = format!(
            "SELECT {COLS} FROM {TABLE} \
             WHERE user_id = {uid} AND revoked = FALSE \
             UNION \
             SELECT {COLS} FROM {TABLE} v \
             INNER JOIN evotai_meta.resource_subscriptions s \
               ON s.resource_type = 'variable' AND s.resource_key = v.id AND s.user_id = {uid} \
             WHERE v.scope = 'shared' AND v.user_id != {uid} AND v.revoked = FALSE AND s.revoked = FALSE \
             ORDER BY created_at DESC LIMIT {lim}",
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_variable).collect()
    }

    async fn list_all(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>> {
        let lim = limit.min(MAX_LIST_LIMIT);
        let stmt = format!(
            "SELECT {COLS} FROM {TABLE} \
             WHERE user_id = {} ORDER BY created_at DESC LIMIT {lim}",
            SqlVal::Str(user_id).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_variable).collect()
    }

    async fn list_shared(&self, user_id: &str, limit: u32) -> Result<Vec<Variable>> {
        let lim = limit.min(MAX_LIST_LIMIT);
        let stmt = format!(
            "SELECT {COLS} FROM {TABLE} \
             WHERE scope = 'shared' AND user_id != {} AND revoked = FALSE \
             ORDER BY created_at DESC LIMIT {lim}",
            SqlVal::Str(user_id).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_variable).collect()
    }

    async fn update(
        &self,
        user_id: &str,
        id: &str,
        key: &str,
        value: &str,
        secret: bool,
        revoked: bool,
    ) -> Result<()> {
        let stmt = format!(
            "UPDATE {TABLE} SET key={}, value={}, secret={}, revoked={}, updated_at=NOW() \
             WHERE id={} AND user_id={}",
            SqlVal::Str(key).render(),
            SqlVal::Str(value).render(),
            secret,
            revoked,
            SqlVal::Str(id).render(),
            SqlVal::Str(user_id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn delete(&self, user_id: &str, id: &str) -> Result<()> {
        let stmt = format!(
            "DELETE FROM {TABLE} WHERE id = {} AND user_id = {}",
            SqlVal::Str(id).render(),
            SqlVal::Str(user_id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn touch_last_used(&self, id: &str, agent_id: &str) -> Result<()> {
        let stmt = format!(
            "UPDATE {TABLE} SET last_used_at=NOW(), last_used_by={} WHERE id={}",
            SqlVal::Str(agent_id).render(),
            SqlVal::Str(id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn touch_last_used_many(&self, ids: &[String], agent_id: &str) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let in_list: String = ids
            .iter()
            .map(|id| SqlVal::Str(id).render())
            .collect::<Vec<_>>()
            .join(", ");
        let stmt = format!(
            "UPDATE {TABLE} SET last_used_at=NOW(), last_used_by={} WHERE id IN ({in_list})",
            SqlVal::Str(agent_id).render(),
        );
        self.pool.exec(&stmt).await
    }
}
