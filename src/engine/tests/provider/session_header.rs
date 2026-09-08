//! Session identity must not depend on the upstream prompt-cache capability.

use evotengine::provider::ApiProtocol;
use evotengine::provider::OpenAiCompat;
use evotengine::provider::OpenAiCompatProvider;
use evotengine::provider::OpenAiResponsesProvider;
use evotengine::provider::StreamProvider;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

use super::fixtures::sse::openai as openai_sse;
use super::fixtures::stream_config::resolved_model_config;
use super::fixtures::stream_config::StreamConfigBuilder;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn check_session_headers(protocol: ApiProtocol, provider: &dyn StreamProvider) -> TestResult {
    let (endpoint, sse) = match protocol {
        ApiProtocol::OpenAiResponses => (
            "/responses",
            concat!(
                "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg\",\"role\":\"assistant\",\"content\":[]}}\n\n",
                "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"ok\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
            ).to_string(),
        ),
        _ => (
            "/chat/completions",
            openai_sse::body(vec![
                openai_sse::text_chunk("ok", None),
                openai_sse::finish_with_usage("stop", 1, 1),
                openai_sse::done(),
            ]),
        ),
    };
    for cache_supported in [false, true] {
        for session in [None, Some(""), Some("session-123")] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(sse.as_str(), "text/event-stream"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let compat = if cache_supported {
                OpenAiCompat::openai()
            } else {
                OpenAiCompat::default()
            };
            let model = resolved_model_config(
                protocol,
                "evot-pro-openai",
                "test-model",
                &server.uri(),
                Some(compat),
                Default::default(),
                Default::default(),
            );
            let mut config = StreamConfigBuilder::openai()
                .model_config(model)
                .cache_disabled();
            if let Some(session) = session {
                config = config.prompt_cache_key(session);
            }
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            provider
                .stream(config.build(), tx, CancellationToken::new())
                .await?;
            let requests = server
                .received_requests()
                .await
                .ok_or("request recording unavailable")?;
            assert_eq!(requests.len(), 1);
            let request = &requests[0];
            assert_eq!(
                request
                    .headers
                    .get("session-id")
                    .map(|value| value.to_str())
                    .transpose()?,
                session.filter(|value| !value.is_empty()),
                "cache_supported={cache_supported}, session={session:?}",
            );
            let body: serde_json::Value = serde_json::from_slice(&request.body)?;
            if cache_supported {
                assert_eq!(
                    body.get("prompt_cache_key"),
                    session.map(|value| serde_json::json!(value)).as_ref()
                );
            } else {
                assert!(body.get("prompt_cache_key").is_none());
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn openai_compat_session_header_is_independent_of_cache_support() -> TestResult {
    check_session_headers(ApiProtocol::OpenAiCompletions, &OpenAiCompatProvider).await
}

#[tokio::test]
async fn responses_session_header_is_independent_of_cache_support() -> TestResult {
    check_session_headers(ApiProtocol::OpenAiResponses, &OpenAiResponsesProvider).await
}
