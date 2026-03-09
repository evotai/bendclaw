//! Databend tool action parsing and SQL generation.

use serde_json::Value;

use crate::storage::sql::escape;
use crate::storage::sql::escape_ident;

/// Actions supported by the Databend tool.
#[derive(Debug)]
pub enum Action {
    Query(String),
    Exec(String),
    Describe {
        table: String,
        database: Option<String>,
    },
    ShowDatabases,
    ShowTables {
        database: Option<String>,
        filter: Option<String>,
    },
    ShowStages,
    ShowFunctions {
        filter: Option<String>,
    },
}

impl Action {
    /// Parse an action from tool call arguments.
    pub fn parse(args: &Value) -> Result<Self, String> {
        let action = str_field(args, "action").unwrap_or_default();
        match action.as_str() {
            "query" => Ok(Self::Query(require(args, "sql")?)),
            "exec" => Ok(Self::Exec(require(args, "sql")?)),
            "describe" => Ok(Self::Describe {
                table: require(args, "table")?,
                database: opt(args, "database"),
            }),
            "show_databases" => Ok(Self::ShowDatabases),
            "show_tables" => Ok(Self::ShowTables {
                database: opt(args, "database"),
                filter: opt(args, "filter"),
            }),
            "show_stages" => Ok(Self::ShowStages),
            "show_functions" => Ok(Self::ShowFunctions {
                filter: opt(args, "filter"),
            }),
            "" => Err("'action' is required".into()),
            other => Err(format!("unknown action: {other}")),
        }
    }

    /// Generate the SQL statement for this action.
    pub fn to_sql(&self) -> String {
        match self {
            Self::Query(sql) | Self::Exec(sql) => sql.clone(),
            Self::Describe { table, database } => match database {
                Some(db) => format!("DESCRIBE `{}`.`{}`", escape_ident(db), escape_ident(table)),
                None => format!("DESCRIBE `{}`", escape_ident(table)),
            },
            Self::ShowDatabases => "SHOW DATABASES".into(),
            Self::ShowTables { database, filter } => {
                let mut sql = "SHOW TABLES".to_string();
                if let Some(db) = database {
                    sql.push_str(&format!(" FROM `{}`", escape_ident(db)));
                }
                if let Some(f) = filter {
                    sql.push_str(&format!(" LIKE '{}'", escape(f)));
                }
                sql
            }
            Self::ShowStages => "SHOW STAGES".into(),
            Self::ShowFunctions { filter } => {
                let mut sql = "SHOW FUNCTIONS".to_string();
                if let Some(f) = filter {
                    sql.push_str(&format!(" LIKE '{}'", escape(f)));
                }
                sql
            }
        }
    }

    /// Whether this action returns a result set.
    pub fn returns_rows(&self) -> bool {
        !matches!(self, Self::Exec(_))
    }
}

// ─── Argument helpers ─────────────────────────────────────────────────────────

fn str_field(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn require(args: &Value, key: &str) -> Result<String, String> {
    str_field(args, key).ok_or_else(|| format!("'{key}' is required"))
}

fn opt(args: &Value, key: &str) -> Option<String> {
    str_field(args, key)
}
