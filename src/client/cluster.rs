use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;

use crate::client::cluster_diagnostics;
use crate::client::http_adapter;
use crate::types::http;
use crate::types::ErrorCode;
use crate::types::Result;

/// Client for the evot-ai cluster registry API.
/// Handles node registration, heartbeat, discovery, and deregistration.
pub struct ClusterClient {
    client: reqwest::Client,
    base_url: String,
    api_token: String,
    node_id: String,
    endpoint: String,
    cluster_id: String,
}

/// Registry-side node record — only routing-essential fields.
/// Business data lives in the opaque `data` blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeEntry {
    pub node_id: String,
    pub endpoint: String,
    pub cluster_id: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl NodeEntry {
    /// Deserialize the opaque `data` blob into [`NodeMeta`].
    /// Returns `Default` on missing or malformed data.
    pub fn meta(&self) -> NodeMeta {
        serde_json::from_value(self.data.clone()).unwrap_or_default()
    }
}

/// Client-side business metadata stored inside `NodeEntry.data`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMeta {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub max_load: u32,
    #[serde(default)]
    pub current_load: u32,
    #[serde(default)]
    pub status: String,
}

#[derive(Serialize)]
struct RegisterRequest<'a> {
    node_id: &'a str,
    endpoint: &'a str,
    cluster_id: &'a str,
    data: serde_json::Value,
}

impl ClusterClient {
    pub fn new(
        base_url: impl Into<String>,
        api_token: impl Into<String>,
        node_id: impl Into<String>,
        endpoint: impl Into<String>,
        cluster_id: impl Into<String>,
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
            node_id: node_id.into(),
            endpoint: endpoint.into(),
            cluster_id: cluster_id.into(),
        }
    }
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn cluster_id(&self) -> &str {
        &self.cluster_id
    }

    /// Register this node with the cluster registry.
    pub async fn register(&self) -> Result<()> {
        let url = format!("{}/v1/cluster/nodes", self.base_url);
        let meta = NodeMeta {
            version: env!("CARGO_PKG_VERSION").to_string(),
            max_load: 10,
            current_load: 0,
            status: "READY".to_string(),
        };
        let body = RegisterRequest {
            node_id: &self.node_id,
            endpoint: &self.endpoint,
            cluster_id: &self.cluster_id,
            data: serde_json::to_value(&meta).unwrap_or_default(),
        };
        let resp = http::send(
            self.client
                .post(&url)
                .bearer_auth(&self.api_token)
                .json(&body),
            http::HttpRequestContext::new("client", "cluster_registration")
                .with_endpoint("cluster_registry")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_registration("cluster_registration", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_registration_read_error")
                    .with_endpoint("cluster_registry")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::cluster_registration(format!(
                "register failed: HTTP {status}: {text}"
            )));
        }

        Ok(())
    }

    /// Send a heartbeat to the cluster registry.
    pub async fn heartbeat(&self) -> Result<()> {
        let url = format!(
            "{}/v1/cluster/nodes/{}/heartbeat",
            self.base_url, self.node_id
        );
        let resp = http::send(
            self.client.put(&url).bearer_auth(&self.api_token),
            http::HttpRequestContext::new("client", "cluster_registration")
                .with_endpoint("cluster_registry")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_registration("cluster_registration", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_registration_read_error")
                    .with_endpoint("cluster_registry")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::cluster_registration(format!(
                "heartbeat failed: HTTP {status}: {text}"
            )));
        }
        Ok(())
    }

    /// Discover peer nodes, filtering out this instance and scoping to the same cluster_id.
    pub async fn discover(&self) -> Result<Vec<NodeEntry>> {
        let url = format!(
            "{}/v1/cluster/nodes?cluster_id={}",
            self.base_url, self.cluster_id
        );
        let resp = http::send(
            self.client.get(&url).bearer_auth(&self.api_token),
            http::HttpRequestContext::new("client", "cluster_discovery")
                .with_endpoint("cluster_registry")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_discovery("cluster_discovery", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_discovery_read_error")
                    .with_endpoint("cluster_registry")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            return Err(ErrorCode::cluster_discovery(format!(
                "discover failed: HTTP {status}: {text}"
            )));
        }

        let nodes: Vec<NodeEntry> = http::read_json(
            resp,
            http::HttpRequestContext::new("client", "cluster_discovery_decode")
                .with_endpoint("cluster_registry")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_discovery("cluster_discovery_decode", err))?;

        Ok(nodes
            .into_iter()
            .filter(|n| n.node_id != self.node_id)
            .collect())
    }

    /// Deregister this node from the cluster registry.
    pub async fn deregister(&self) -> Result<()> {
        let url = format!("{}/v1/cluster/nodes/{}", self.base_url, self.node_id);
        let resp = http::send(
            self.client.delete(&url).bearer_auth(&self.api_token),
            http::HttpRequestContext::new("client", "cluster_registration")
                .with_endpoint("cluster_registry")
                .with_url(url.clone()),
        )
        .await
        .map_err(|err| http_adapter::to_cluster_registration("cluster_registration", err))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = http::read_text(
                resp,
                http::HttpRequestContext::new("client", "cluster_registration_read_error")
                    .with_endpoint("cluster_registry")
                    .with_url(url.clone()),
            )
            .await
            .unwrap_or_default();
            cluster_diagnostics::log_cluster_client_deregister_failed(&self.node_id, status, &text);
        }
        Ok(())
    }
}
