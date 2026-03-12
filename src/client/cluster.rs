use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::base::ErrorCode;
use crate::base::Result;

/// Client for the evot-ai cluster registry API.
/// Handles node registration, heartbeat, discovery, and deregistration.
pub struct ClusterClient {
    client: reqwest::Client,
    base_url: String,
    api_token: String,
    instance_id: String,
    endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub instance_id: String,
    pub endpoint: String,
    pub max_load: u32,
    pub current_load: u32,
    pub status: String,
}

#[derive(Serialize)]
struct RegisterRequest<'a> {
    instance_id: &'a str,
    endpoint: &'a str,
    max_load: u32,
}

impl ClusterClient {
    pub fn new(
        base_url: impl Into<String>,
        api_token: impl Into<String>,
        instance_id: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_token: api_token.into(),
            instance_id: instance_id.into(),
            endpoint: endpoint.into(),
        }
    }
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Register this node with the cluster registry.
    pub async fn register(&self) -> Result<()> {
        let url = format!("{}/v1/cluster/nodes", self.base_url);
        let body = RegisterRequest {
            instance_id: &self.instance_id,
            endpoint: &self.endpoint,
            max_load: 10,
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                ErrorCode::cluster_registration(format!("register request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::cluster_registration(format!(
                "register failed: HTTP {status}: {text}"
            )));
        }
        tracing::info!(instance_id = %self.instance_id, "registered with cluster");
        Ok(())
    }

    /// Send a heartbeat to the cluster registry.
    pub async fn heartbeat(&self) -> Result<()> {
        let url = format!(
            "{}/v1/cluster/nodes/{}/heartbeat",
            self.base_url, self.instance_id
        );
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map_err(|e| {
                ErrorCode::cluster_registration(format!("heartbeat request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::cluster_registration(format!(
                "heartbeat failed: HTTP {status}: {text}"
            )));
        }
        Ok(())
    }

    /// Discover peer nodes, filtering out this instance.
    pub async fn discover(&self) -> Result<Vec<NodeInfo>> {
        let url = format!("{}/v1/cluster/nodes", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map_err(|e| ErrorCode::cluster_discovery(format!("discover request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ErrorCode::cluster_discovery(format!(
                "discover failed: HTTP {status}: {text}"
            )));
        }

        let nodes: Vec<NodeInfo> = resp
            .json()
            .await
            .map_err(|e| ErrorCode::cluster_discovery(format!("failed to parse nodes: {e}")))?;

        Ok(nodes
            .into_iter()
            .filter(|n| n.instance_id != self.instance_id)
            .collect())
    }

    /// Deregister this node from the cluster registry.
    pub async fn deregister(&self) -> Result<()> {
        let url = format!("{}/v1/cluster/nodes/{}", self.base_url, self.instance_id);
        let resp = self
            .client
            .delete(&url)
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map_err(|e| {
                ErrorCode::cluster_registration(format!("deregister request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            tracing::warn!(
                instance_id = %self.instance_id,
                "deregister failed: HTTP {status}: {text}"
            );
        } else {
            tracing::info!(instance_id = %self.instance_id, "deregistered from cluster");
        }
        Ok(())
    }
}
