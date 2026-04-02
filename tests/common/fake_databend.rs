#![allow(dead_code)]

use std::sync::Arc;
use std::sync::Mutex;

use bendclaw::storage::pool::ApiError;
use bendclaw::storage::pool::DatabendClient;
use bendclaw::storage::pool::QueryResponse;
use bendclaw::storage::pool::SchemaField;
use bendclaw::storage::Pool;
use bendclaw::types::ErrorCode;
use bendclaw::types::Result;
use databend_common_ast::parser::parse_sql;
use databend_common_ast::parser::tokenize_sql;
use databend_common_ast::parser::Dialect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeDatabendCall {
    Query {
        sql: String,
        database: Option<String>,
    },
    Page {
        uri: String,
    },
    Finalize {
        uri: String,
    },
}

type QueryHandler = dyn Fn(&str, Option<&str>) -> Result<QueryResponse> + Send + Sync;
type PageHandler = dyn Fn(&str) -> Result<QueryResponse> + Send + Sync;
type FinalizeHandler = dyn Fn(&str) -> Result<()> + Send + Sync;

#[derive(Clone)]
pub struct FakeDatabend {
    calls: Arc<Mutex<Vec<FakeDatabendCall>>>,
    query: Arc<QueryHandler>,
    page: Arc<PageHandler>,
    finalize: Arc<FinalizeHandler>,
}

impl FakeDatabend {
    pub fn new(
        query: impl Fn(&str, Option<&str>) -> Result<QueryResponse> + Send + Sync + 'static,
    ) -> Self {
        Self::with_handlers(
            query,
            |_uri| {
                Err(ErrorCode::storage_exec(
                    "unexpected page request".to_string(),
                ))
            },
            |_uri| Ok(()),
        )
    }

    pub fn with_handlers(
        query: impl Fn(&str, Option<&str>) -> Result<QueryResponse> + Send + Sync + 'static,
        page: impl Fn(&str) -> Result<QueryResponse> + Send + Sync + 'static,
        finalize: impl Fn(&str) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            query: Arc::new(query),
            page: Arc::new(page),
            finalize: Arc::new(finalize),
        }
    }

    pub fn calls(&self) -> Vec<FakeDatabendCall> {
        self.calls.lock().expect("fake databend calls lock").clone()
    }

    pub fn pool(&self) -> Pool {
        Pool::from_client("http://fake.local/v1", "default", Arc::new(self.clone()))
    }
}

/// Validate SQL syntax using Databend's own parser. Panics on parse failure.
/// Skips very large SQL (>4KB) to avoid stack overflow in the recursive parser.
fn assert_valid_sql(sql: &str) {
    if sql.len() > 4096 {
        return;
    }
    let tokens = tokenize_sql(sql).unwrap_or_else(|e| {
        panic!("SQL tokenize error in query:\n  {sql}\n  Error: {e}");
    });
    if let Err(e) = parse_sql(&tokens, Dialect::Experimental) {
        panic!("SQL syntax error in query:\n  {sql}\n  Error: {e}");
    }
}

#[async_trait::async_trait]
impl DatabendClient for FakeDatabend {
    async fn query(&self, sql: &str, database: Option<&str>) -> Result<QueryResponse> {
        assert_valid_sql(sql);
        self.calls
            .lock()
            .expect("fake databend calls lock")
            .push(FakeDatabendCall::Query {
                sql: sql.to_string(),
                database: database.map(ToOwned::to_owned),
            });
        (self.query)(sql, database)
    }

    async fn page(&self, uri: &str) -> Result<QueryResponse> {
        self.calls
            .lock()
            .expect("fake databend calls lock")
            .push(FakeDatabendCall::Page {
                uri: uri.to_string(),
            });
        (self.page)(uri)
    }

    async fn finalize(&self, uri: &str) -> Result<()> {
        self.calls
            .lock()
            .expect("fake databend calls lock")
            .push(FakeDatabendCall::Finalize {
                uri: uri.to_string(),
            });
        (self.finalize)(uri)
    }
}

pub fn rows(data: &[&[&str]]) -> QueryResponse {
    QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: data
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| serde_json::Value::String((*value).to_string()))
                    .collect()
            })
            .collect(),
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

pub fn paged_rows(
    data: &[&[&str]],
    next_uri: Option<&str>,
    final_uri: Option<&str>,
) -> QueryResponse {
    QueryResponse {
        next_uri: next_uri.map(ToOwned::to_owned),
        final_uri: final_uri.map(ToOwned::to_owned),
        ..rows(data)
    }
}

pub fn api_error(code: i64, message: &str) -> QueryResponse {
    QueryResponse {
        id: String::new(),
        state: "Failed".to_string(),
        error: Some(ApiError {
            code,
            message: message.to_string(),
        }),
        data: Vec::new(),
        next_uri: None,
        final_uri: None,
        schema: vec![SchemaField {
            name: String::new(),
            field_type: String::new(),
        }],
    }
}
