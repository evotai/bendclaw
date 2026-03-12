use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::base::ErrorCode;
use crate::base::Result;

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
    pub async fn create_run(
        &self,
        endpoint: &str,
        agent_id: &str,
        input: &str,
        user_id: &str,
        parent_run_id: Option<&str>,
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
        let resp = req.json(&body).send().await.map_err(|e| {
            ErrorCode::cluster_dispatch(format!("create_run request to {endpoint} failed: {e}"))
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::cluster_dispatch(format!(
                "create_run failed on {endpoint}: HTTP {status}: {text}"
            )));
        }

        resp.json().await.map_err(|e| {
            ErrorCode::cluster_dispatch(format!("failed to parse create_run response: {e}"))
        })
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
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.auth_token)
            .header("x-user-id", user_id)
            .send()
            .await
            .map_err(|e| {
                ErrorCode::cluster_collect(format!("get_run request to {endpoint} failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::cluster_collect(format!(
                "get_run failed on {endpoint}: HTTP {status}: {text}"
            )));
        }

        resp.json().await.map_err(|e| {
            ErrorCode::cluster_collect(format!("failed to parse get_run response: {e}"))
        })
    }
}
