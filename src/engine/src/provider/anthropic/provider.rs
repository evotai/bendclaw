//! Anthropic Claude provider (Messages API with streaming + JSON fallback).

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::debug;

use super::json_fallback;
use super::request;
use super::sse_decode;
use crate::provider::error::*;
use crate::provider::stream_http::StreamResponseKind;
use crate::provider::stream_http::{self};
use crate::provider::traits::*;
use crate::types::*;

const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider;

#[async_trait]
impl StreamProvider for AnthropicProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        let is_oauth = config.api_key.contains("sk-ant-oat");

        let base_url = config
            .model_config
            .as_ref()
            .map(|mc| mc.base_url.trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| "https://api.anthropic.com".into());

        let is_custom = base_url != "https://api.anthropic.com";
        let url = format!("{}/v1/messages", base_url);

        let body = request::build_request_body(&config, is_oauth);
        debug!(
            "Anthropic request: model={}, oauth={}, url={}",
            config.model, is_oauth, url
        );

        let client = crate::provider::error::new_client()?;
        let mut builder = client.post(&url).header("content-type", "application/json");

        if is_custom {
            // Custom endpoint — Bearer auth, no Anthropic-specific headers
            builder = builder.header("authorization", format!("Bearer {}", config.api_key));
        } else if is_oauth {
            // Official endpoint, OAuth token
            builder = builder
                .header("authorization", format!("Bearer {}", config.api_key))
                .header(
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14",
                )
                .header("anthropic-dangerous-direct-browser-access", "true")
                .header("user-agent", "claude-cli/2.1.2 (external, cli)")
                .header("x-app", "cli");
        } else {
            // Official endpoint, API key
            builder = builder
                .header("anthropic-version", API_VERSION)
                .header("x-api-key", &config.api_key);
        }

        // Extra headers from model config (e.g. custom auth overrides)
        if let Some(mc) = &config.model_config {
            for (k, v) in &mc.headers {
                builder = builder.header(k, v);
            }
        }

        let builder = builder.json(&body);

        // Send request and check HTTP status
        let response = stream_http::send_stream_request(builder).await?;
        let response = stream_http::check_error_status(response).await?;

        // Classify response by content-type
        let kind = stream_http::classify_response(&response);
        debug!("Anthropic response kind: {kind:?}");

        match kind {
            StreamResponseKind::Streaming => {
                sse_decode::decode_sse_stream(response, tx, cancel, &config).await
            }
            StreamResponseKind::Json => {
                json_fallback::handle_json_response(response, tx, &config).await
            }
            StreamResponseKind::Other(ct) => Err(ProviderError::Api(format!(
                "Unexpected content type from Anthropic: {ct}"
            ))),
        }
    }
}
