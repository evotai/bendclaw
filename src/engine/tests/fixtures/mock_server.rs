//! Wiremock-based mock server runners for provider integration tests.

use evotengine::provider::error::ProviderError;
use evotengine::provider::traits::*;
use evotengine::provider::ModelConfig;
use evotengine::provider::ModelOverrides;
use evotengine::provider::ResolveModelRequest;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

use super::stream_config::collect_stream_events;

/// Run a provider against a wiremock server returning SSE events.
/// Returns (Message, Vec<StreamEvent>) or ProviderError.
pub async fn run_provider_sse(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    sse_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("content-type", "text/event-stream")
                .insert_header("cache-control", "no-cache")
                .set_body_raw(sse_body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|outcome| (outcome.into_message(), events))
}

pub async fn run_provider_sse_outcome(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    sse_body: &str,
    status: u16,
) -> Result<(StreamOutcome, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("content-type", "text/event-stream")
                .insert_header("cache-control", "no-cache")
                .set_body_raw(sse_body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|outcome| (outcome, events))
}

/// Run a provider against a wiremock server returning JSON.
pub async fn run_provider_json(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    json_body: &str,
    status: u16,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status).set_body_raw(json_body.to_string(), "application/json"),
        )
        .mount(&server)
        .await;

    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();

    let result = provider.stream(config, tx, cancel).await;
    let events = collect_stream_events(&mut rx);

    result.map(|outcome| (outcome.into_message(), events))
}

/// Exercise the bounded provider path with a capacity-one consumer.
pub async fn run_provider_sse_bounded(
    provider: &dyn StreamProvider,
    config: StreamConfig,
    body: &str,
) -> Result<(Message, Vec<StreamEvent>), ProviderError> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream"),
        )
        .mount(&server)
        .await;
    let config = override_base_url(config, &server.uri());
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let collect = async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
            tokio::task::yield_now().await;
        }
        events
    };
    let (outcome, events) = tokio::join!(
        provider.stream_bounded(config, tx, CancellationToken::new()),
        collect
    );
    outcome.map(|outcome| (outcome.into_message(), events))
}

/// Override the base_url in a StreamConfig's model_config to point at the mock server.
/// For Anthropic (no model_config), creates one with the given base_url.
fn override_base_url(mut config: StreamConfig, base_url: &str) -> StreamConfig {
    let model_config = match config.model_config.take() {
        Some(existing) => ModelConfig::resolve(ResolveModelRequest {
            protocol: existing.protocol(),
            provider: existing.provider().to_string(),
            model_id: existing.id().to_string(),
            base_url: base_url.to_string(),
            headers: existing.headers().clone(),
            compat: existing.compat().cloned(),
            route_capabilities: existing.route_capabilities(),
            overrides: ModelOverrides {
                context_window: Some(existing.context_window()),
                max_output_tokens: Some(existing.max_tokens()),
                supports_image: Some(existing.supports_image()),
                reasoning: Some(existing.reasoning()),
            },
        }),
        None => ModelConfig::resolve(ResolveModelRequest {
            protocol: evotengine::provider::ApiProtocol::AnthropicMessages,
            provider: "anthropic".into(),
            model_id: "test-model".into(),
            base_url: base_url.to_string(),
            headers: Default::default(),
            compat: None,
            route_capabilities: Default::default(),
            overrides: ModelOverrides::default(),
        }),
    };
    config.model_config = Some(model_config);
    config
}
