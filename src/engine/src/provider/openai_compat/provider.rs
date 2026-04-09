//! OpenAI Chat Completions compatible provider.
//!
//! One implementation covers OpenAI, xAI, Groq, Cerebras, OpenRouter,
//! Mistral, DeepSeek, MiniMax, HuggingFace, Kimi, and any other provider
//! that implements the OpenAI Chat Completions API.
//!
//! Behavioral differences are handled via `OpenAiCompat` flags in ModelConfig.

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::debug;

use super::json_fallback;
use super::request;
use super::sse_decode;
use crate::provider::stream_http::StreamResponseKind;
use crate::provider::stream_http::{self};
use crate::provider::traits::*;
use crate::types::*;

pub struct OpenAiCompatProvider;

#[async_trait]
impl StreamProvider for OpenAiCompatProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        let model_config = config.model_config.as_ref().ok_or_else(|| {
            ProviderError::Other("ModelConfig required for OpenAI provider".into())
        })?;
        let compat = model_config.compat.as_ref().cloned().unwrap_or_default();

        let base_url = &model_config.base_url;
        let url = format!("{}/chat/completions", base_url);

        let body = request::build_request_body(&config, model_config, &compat);
        debug!("OpenAI compat request: model={} url={}", config.model, url);

        let client = reqwest::Client::new();
        let mut builder = client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", config.api_key));

        // Add any extra headers from model config
        for (k, v) in &model_config.headers {
            builder = builder.header(k, v);
        }

        let builder = builder.json(&body);

        // Send request and check HTTP status
        let response = stream_http::send_stream_request(builder).await?;
        let response = stream_http::check_error_status(response).await?;

        // Classify response by content-type
        let kind = stream_http::classify_response(&response);
        debug!("OpenAI compat response kind: {kind:?}");

        match kind {
            StreamResponseKind::Streaming => {
                sse_decode::decode_sse_stream(response, tx, cancel, &config, &compat).await
            }
            StreamResponseKind::Json => {
                json_fallback::handle_json_response(response, tx, &config, &compat).await
            }
            StreamResponseKind::Other(ct) => Err(ProviderError::Api(format!(
                "Unexpected content type from OpenAI-compatible endpoint: {ct}"
            ))),
        }
    }
}
