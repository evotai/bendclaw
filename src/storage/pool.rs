//! Databend Cloud HTTP API connection pool.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use backon::ExponentialBuilder;
use backon::Retryable;
use serde::Deserialize;
use serde::Serialize;

use crate::base::ErrorCode;
use crate::base::Result;

const QUERY_TIMEOUT: Duration = Duration::from_secs(60);

#[async_trait::async_trait]
pub trait DatabendClient: Send + Sync {
    async fn query(&self, sql: &str, database: Option<&str>) -> Result<QueryResponse>;
    async fn page(&self, uri: &str) -> Result<QueryResponse>;
    async fn finalize(&self, uri: &str) -> Result<()>;
}

#[derive(Clone)]
struct HttpDatabendClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
    warehouse: String,
}

impl HttpDatabendClient {
    fn new(base_url: &str, token: &str, warehouse: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(QUERY_TIMEOUT)
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(10)
            .build()
            .map_err(|e| {
                ErrorCode::storage_connection(format!("failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            client,
            base_url: normalize_base_url(base_url),
            token: token.to_string(),
            warehouse: warehouse.to_string(),
        })
    }
}

#[async_trait::async_trait]
impl DatabendClient for HttpDatabendClient {
    async fn query(&self, sql: &str, database: Option<&str>) -> Result<QueryResponse> {
        let url = format!("{}/query", self.base_url);
        let session = database.map(|db| {
            let mut data = serde_json::Map::new();
            data.insert(
                "database".to_string(),
                serde_json::Value::String(db.to_string()),
            );
            serde_json::Value::Object(data)
        });
        let body = QueryRequest {
            sql: sql.to_string(),
            string_fields: true,
            session,
        };

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-DatabendCloud-Token", &self.token)
            .header("X-DatabendCloud-Warehouse", &self.warehouse)
            .json(&body)
            .send()
            .await
            .map_err(|e| classify_reqwest_error(e, "query"))?;

        parse_response(resp, "query").await
    }

    async fn page(&self, uri: &str) -> Result<QueryResponse> {
        let url = resolve_url(&self.base_url, uri);
        let resp = self
            .client
            .get(&url)
            .header("X-DatabendCloud-Token", &self.token)
            .header("X-DatabendCloud-Warehouse", &self.warehouse)
            .send()
            .await
            .map_err(|e| classify_reqwest_error(e, "page"))?;

        parse_response(resp, "page").await
    }

    async fn finalize(&self, uri: &str) -> Result<()> {
        let url = resolve_url(&self.base_url, uri);
        if let Err(error) = self
            .client
            .get(&url)
            .header("X-DatabendCloud-Token", &self.token)
            .header("X-DatabendCloud-Warehouse", &self.warehouse)
            .send()
            .await
        {
            tracing::warn!(url = %url, error = %error, "finalize request failed");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Pool {
    client: Arc<dyn DatabendClient>,
    base_url: String,
    warehouse: String,
    database: Option<String>,
}

impl Pool {
    pub fn new(base_url: &str, token: &str, warehouse: &str) -> Result<Self> {
        let base_url = normalize_base_url(base_url);
        let client = Arc::new(HttpDatabendClient::new(&base_url, token, warehouse)?);
        Ok(Self::from_client(&base_url, warehouse, client))
    }

    pub fn from_client(base_url: &str, warehouse: &str, client: Arc<dyn DatabendClient>) -> Self {
        Self {
            client,
            base_url: normalize_base_url(base_url),
            warehouse: warehouse.to_string(),
            database: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn exec(&self, sql: &str) -> Result<()> {
        let sql = sql.to_string();
        let started = Instant::now();
        tracing::debug!(
            stage = "storage",
            operation = "exec",
            status = "started",
            base_url = %self.base_url,
            warehouse = %self.warehouse,
            database = self.database.as_deref().unwrap_or_default(),
            sql = %sql,
            sql_bytes = sql.len(),
            "storage query"
        );
        let op = || async {
            let resp = self.do_query(&sql).await?;
            if let Some(ref err) = resp.error {
                if err.code != 0 {
                    return Err(classify_api_error(err));
                }
            }
            self.finalize(&resp).await?;
            Ok(())
        };

        let result = op
            .retry(backoff_builder())
            .when(is_retryable)
            .notify(|e: &ErrorCode, dur: Duration| {
                tracing::warn!(
                    stage = "storage",
                    operation = "exec",
                    status = "retrying",
                    base_url = %self.base_url,
                    warehouse = %self.warehouse,
                    database = self.database.as_deref().unwrap_or_default(),
                    sql = %sql,
                    error = %e,
                    delay_ms = dur.as_millis() as u64,
                    "storage query"
                );
            })
            .await;
        match &result {
            Ok(_) => tracing::debug!(
                stage = "storage",
                operation = "exec",
                status = "completed",
                base_url = %self.base_url,
                warehouse = %self.warehouse,
                database = self.database.as_deref().unwrap_or_default(),
                sql = %sql,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "storage query"
            ),
            Err(error) => tracing::error!(
                stage = "storage",
                operation = "exec",
                status = "failed",
                base_url = %self.base_url,
                warehouse = %self.warehouse,
                database = self.database.as_deref().unwrap_or_default(),
                sql = %sql,
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %error,
                "storage query"
            ),
        }
        result
    }

    pub async fn query_all(&self, sql: &str) -> Result<Vec<serde_json::Value>> {
        let sql = sql.to_string();
        let started = Instant::now();
        tracing::debug!(
            stage = "storage",
            operation = "query_all",
            status = "started",
            base_url = %self.base_url,
            warehouse = %self.warehouse,
            database = self.database.as_deref().unwrap_or_default(),
            sql = %sql,
            sql_bytes = sql.len(),
            "storage query"
        );
        let op = || async {
            let resp = self.do_query(&sql).await?;
            if let Some(ref err) = resp.error {
                if err.code != 0 {
                    return Err(classify_api_error(err));
                }
            }

            let mut all_rows = convert_rows(&resp);
            let mut next = resp.next_uri.clone();
            let final_uri = resp.final_uri.clone();

            while let Some(ref uri) = next {
                if uri.is_empty() {
                    break;
                }
                let page = self.do_get_page(uri).await?;
                if let Some(ref err) = page.error {
                    if err.code != 0 {
                        return Err(classify_api_error(err));
                    }
                }
                all_rows.extend(convert_rows(&page));
                next = page.next_uri.clone();
            }

            if let Some(ref uri) = final_uri {
                self.do_final(uri).await?;
            }

            Ok(all_rows)
        };

        let result = op
            .retry(backoff_builder())
            .when(is_retryable)
            .notify(|e: &ErrorCode, dur: Duration| {
                tracing::warn!(
                    stage = "storage",
                    operation = "query_all",
                    status = "retrying",
                    base_url = %self.base_url,
                    warehouse = %self.warehouse,
                    database = self.database.as_deref().unwrap_or_default(),
                    sql = %sql,
                    error = %e,
                    delay_ms = dur.as_millis() as u64,
                    "storage query"
                );
            })
            .await;
        match &result {
            Ok(rows) => tracing::debug!(
                stage = "storage",
                operation = "query_all",
                status = "completed",
                base_url = %self.base_url,
                warehouse = %self.warehouse,
                database = self.database.as_deref().unwrap_or_default(),
                sql = %sql,
                rows = rows.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "storage query"
            ),
            Err(error) => tracing::error!(
                stage = "storage",
                operation = "query_all",
                status = "failed",
                base_url = %self.base_url,
                warehouse = %self.warehouse,
                database = self.database.as_deref().unwrap_or_default(),
                sql = %sql,
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %error,
                "storage query"
            ),
        }
        result
    }

    pub async fn query_row(&self, sql: &str) -> Result<Option<serde_json::Value>> {
        self.query_all(sql)
            .await
            .map(|rows| rows.into_iter().next())
    }

    pub fn with_database(&self, db_name: &str) -> Result<Self> {
        Ok(Self {
            client: Arc::clone(&self.client),
            base_url: self.base_url.clone(),
            warehouse: self.warehouse.clone(),
            database: Some(db_name.to_string()),
        })
    }

    async fn do_query(&self, sql: &str) -> Result<QueryResponse> {
        self.client.query(sql, self.database.as_deref()).await
    }

    async fn do_get_page(&self, uri: &str) -> Result<QueryResponse> {
        self.client.page(uri).await
    }

    async fn do_final(&self, uri: &str) -> Result<()> {
        self.client.finalize(uri).await
    }

    async fn finalize(&self, resp: &QueryResponse) -> Result<()> {
        if let Some(ref uri) = resp.final_uri {
            self.do_final(uri).await?;
        }
        Ok(())
    }
}

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Serialize)]
struct QueryRequest {
    sql: String,
    string_fields: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct QueryResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub error: Option<ApiError>,
    #[serde(default)]
    pub data: Vec<Vec<serde_json::Value>>,
    #[serde(default)]
    pub next_uri: Option<String>,
    #[serde(default)]
    pub final_uri: Option<String>,
    #[serde(default)]
    pub schema: Vec<SchemaField>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaField {
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub field_type: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn convert_rows(resp: &QueryResponse) -> Vec<serde_json::Value> {
    resp.data
        .iter()
        .map(|row| {
            let values: Vec<serde_json::Value> = row
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
                    other => serde_json::Value::String(other.to_string()),
                })
                .collect();
            serde_json::Value::Array(values)
        })
        .collect()
}

fn normalize_base_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    if url.ends_with("/v1") || url.ends_with("/v1.1") {
        url.to_string()
    } else {
        format!("{url}/v1")
    }
}

fn resolve_url(base_url: &str, uri: &str) -> String {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return uri.to_string();
    }
    // API returns URIs like /v1/query/{id}/final.
    // Replace the /v1/query prefix with {base_url}/query to go through the correct gateway path.
    if let Some(rest) = uri.strip_prefix("/v1/query") {
        return format!("{base_url}/query{rest}");
    }
    // Fallback: origin + uri
    if let Some(idx) = base_url.find("://") {
        if let Some(slash) = base_url[idx + 3..].find('/') {
            let origin = &base_url[..idx + 3 + slash];
            return format!("{origin}{uri}");
        }
    }
    format!("{base_url}{uri}")
}

async fn parse_response(resp: reqwest::Response, context: &str) -> Result<QueryResponse> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let lower = body.to_lowercase();
        if lower.contains("unknown database")
            || (lower.contains("database") && lower.contains("not") && lower.contains("exist"))
        {
            return Err(ErrorCode::not_found(body));
        }
        return Err(ErrorCode::storage_exec(format!(
            "{context}: HTTP {status}: {body}"
        )));
    }
    resp.json::<QueryResponse>()
        .await
        .map_err(|e| ErrorCode::storage_exec(format!("{context}: failed to parse response: {e}")))
}

fn classify_reqwest_error(e: reqwest::Error, context: &str) -> ErrorCode {
    if e.is_timeout() {
        ErrorCode::timeout(format!("{context}: request timed out"))
    } else if e.is_connect() {
        ErrorCode::storage_connection(format!("{context}: connection failed: {e}"))
    } else {
        ErrorCode::storage_exec(format!("{context}: {e}"))
    }
}

fn classify_api_error(e: &ApiError) -> ErrorCode {
    let lower = e.message.to_lowercase();
    if lower.contains("unknown database")
        || (lower.contains("database") && lower.contains("not") && lower.contains("exist"))
    {
        return ErrorCode::not_found(e.message.clone());
    }
    ErrorCode::storage_exec(format!("code {}: {}", e.code, e.message))
}

fn backoff_builder() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(500))
        .with_max_delay(Duration::from_secs(5))
        .with_max_times(3)
}

fn is_retryable(e: &ErrorCode) -> bool {
    matches!(e.code, ErrorCode::STORAGE_CONNECTION | ErrorCode::TIMEOUT) || {
        let msg = e.message.to_lowercase();
        msg.contains("timeout")
            || msg.contains("connection")
            || msg.contains("broken pipe")
            || msg.contains("reset by peer")
            || msg.contains("temporarily unavailable")
            || msg.contains("unknown database")
    }
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("base_url", &self.base_url)
            .field("warehouse", &self.warehouse)
            .field("database", &self.database)
            .finish()
    }
}
