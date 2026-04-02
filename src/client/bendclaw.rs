use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::client::http_adapter;
use crate::types::http;
use crate::types::ErrorCode;
use crate::types::Result;

/// Client for calling another bendclaw instance's REST API.
/// Uses the shared `auth.api_key` for node-to-node authentication.
pub struct BendclawClient {
    client: reqwest::Client,
    auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRunResponse {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub output: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Serialize)]
struct CreateRunBody<'a> {
    input: &'a str,
    stream: bool,
}

impl BendclawClient {
    pub fn new(auth_token: &str, timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            client,
            auth_token: auth_token.to_string(),
        }
    }

    /// Create a run on a remote bendclaw node.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_run(
        &self,
        endpoint: &str,
        agent_id: &str,
        input: &str,
        user_id: &str,
        parent_run_id: Option<&str>,
        trace_id: Option<&str>,
        origin_node_id: Option<&str>,
    ) -> Result<RemoteRunResponse> {
        let url = format!(
            "{}/v1/agents/{}/runs",
            endpoint.trim_end_matches('/'),
            agent_id
        );
        let body = CreateRunBody {
            input,
            stream: false,
        };
        let mut req = self
            .client
            .post(&url)
            .bearer_auth(&self.auth_token)
            .header("x-user-id", user_id);
        if let Some(prid) = parent_run_id {
            req = req.header("x-parent-run-id", prid);
        }
        if let Some(tid) = trace_id {
            req = req.header("x-trace-id", tid);
        }
        if let Some(onid) = origin_node_id {
            req = req.header("x-origin-node-id", onid);
        }
        let resp = http::send(
            req.json(&body),
            http::HttpRequestContext::new("client", "cluster_dispatch")
                .with_endpoint("bendclaw")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_dispatch("cluster_dispatch", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_dispatch_read_error")
                    .with_endpoint("bendclaw")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::cluster_dispatch(format!(
                "create_run failed on {endpoint}: HTTP {status}: {text}"
            )));
        }

        http::read_json(
            resp,
            http::HttpRequestContext::new("client", "cluster_dispatch_decode")
                .with_endpoint("bendclaw")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_dispatch("cluster_dispatch_decode", err))
    }

    /// Get the status of a run on a remote bendclaw node.
    pub async fn get_run(
        &self,
        endpoint: &str,
        agent_id: &str,
        run_id: &str,
        user_id: &str,
    ) -> Result<RemoteRunResponse> {
        let url = format!(
            "{}/v1/agents/{}/runs/{}",
            endpoint.trim_end_matches('/'),
            agent_id,
            run_id
        );
        let resp = http::send(
            self.client
                .get(&url)
                .bearer_auth(&self.auth_token)
                .header("x-user-id", user_id),
            http::HttpRequestContext::new("client", "cluster_collect")
                .with_endpoint("bendclaw")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_collect("cluster_collect", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_collect_read_error")
                    .with_endpoint("bendclaw")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::cluster_collect(format!(
                "get_run failed on {endpoint}: HTTP {status}: {text}"
            )));
        }

        http::read_json(
            resp,
            http::HttpRequestContext::new("client", "cluster_collect_decode")
                .with_endpoint("bendclaw")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_collect("cluster_collect_decode", err))
    }
}
