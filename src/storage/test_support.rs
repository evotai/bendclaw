#![cfg(any(test, doctest))]

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::pool::DatabendClient;
use super::pool::QueryResponse;
use super::Pool;
use crate::types::ErrorCode;
use crate::types::Result;

type QueryHandler = dyn Fn(&str, Option<&str>) -> Result<QueryResponse> + Send + Sync;
type PageHandler = dyn Fn(&str) -> Result<QueryResponse> + Send + Sync;
type FinalizeHandler = dyn Fn(&str) -> Result<()> + Send + Sync;

#[derive(Clone)]
pub struct RecordingClient {
    sqls: Arc<Mutex<Vec<String>>>,
    query: Arc<QueryHandler>,
    page: Arc<PageHandler>,
    finalize: Arc<FinalizeHandler>,
}

impl RecordingClient {
    pub fn new(
        query: impl Fn(&str, Option<&str>) -> Result<QueryResponse> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sqls: Arc::new(Mutex::new(Vec::new())),
            query: Arc::new(query),
            page: Arc::new(|_uri| Err(ErrorCode::internal("unexpected page request"))),
            finalize: Arc::new(|_uri| Ok(())),
        }
    }

    pub fn sqls(&self) -> Vec<String> {
        self.sqls.lock().clone()
    }

    pub fn pool(&self) -> Pool {
        Pool::from_client("http://fake.local/v1", "default", Arc::new(self.clone()))
    }
}

#[async_trait]
impl DatabendClient for RecordingClient {
    async fn query(&self, sql: &str, database: Option<&str>) -> Result<QueryResponse> {
        self.sqls.lock().push(sql.to_string());
        (self.query)(sql, database)
    }

    async fn page(&self, uri: &str) -> Result<QueryResponse> {
        (self.page)(uri)
    }

    async fn finalize(&self, uri: &str) -> Result<()> {
        (self.finalize)(uri)
    }
}
