use evotengine::provider::OpenAiResponsesProvider;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamProvider;
use evotengine::types::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

use super::super::fixtures::mock_server::run_provider_sse;
use super::super::fixtures::stream_config::*;

fn responses_config(model: &str) -> StreamConfigBuilder {
    StreamConfigBuilder::openai().model_config(evotengine::provider::ModelConfig::openai_responses(
        model, model,
    ))
}

#[tokio::test]
async fn bounded_responses_preserves_terminal_and_delta_order(
) -> Result<(), Box<dyn std::error::Error>> {
    let body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"one\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"two\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}}\n\n",
    );
    let (_, events) = super::super::fixtures::mock_server::run_provider_sse_bounded(
        &OpenAiResponsesProvider,
        responses_config("gpt-5.5").build(),
        body,
    )
    .await?;
    let deltas: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            StreamEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["one", "two"]);
    assert!(matches!(events.last(), Some(StreamEvent::Done { .. })));
    Ok(())
}

#[tokio::test]
async fn rejected_compaction_replay_retries_once_with_fallback_text(
) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(400).set_body_string("invalid compaction item"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let sse = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"recovered\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(sse, "text/event-stream"),
        )
        .mount(&server)
        .await;

    let model = resolved_model_config(
        evotengine::provider::ApiProtocol::OpenAiResponses,
        "openai",
        "gpt-5.5",
        &server.uri(),
        Some(evotengine::provider::OpenAiCompat::openai()),
        evotengine::provider::RouteCapabilities {
            verbosity: true,
            remote_compaction: true,
        },
        Default::default(),
    );
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.5")
        .model_config(model)
        .messages(vec![Message::Assistant {
            content: vec![Content::Thinking {
                thinking: "portable fallback".into(),
                metadata: Some(ThinkingMetadata::OpenAiResponses {
                    item: serde_json::json!({
                        "type": "compaction",
                        "encrypted_content": "opaque",
                    }),
                }),
            }],
            stop_reason: StopReason::Stop,
            model: "gpt-5.5".into(),
            provider: "openai".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }])
        .build();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = OpenAiResponsesProvider
        .stream(config, tx, CancellationToken::new())
        .await?;
    assert!(matches!(
        outcome.message(),
        Message::Assistant { content, .. }
            if matches!(content.first(), Some(Content::Text { text }) if text == "recovered")
    ));

    let requests = server
        .received_requests()
        .await
        .ok_or("request recording unavailable")?;
    assert_eq!(requests.len(), 2);
    let first: serde_json::Value = serde_json::from_slice(&requests[0].body)?;
    let second: serde_json::Value = serde_json::from_slice(&requests[1].body)?;
    assert_eq!(first["input"][0]["type"], "compaction");
    assert_eq!(
        second["input"][0]["content"][0]["text"],
        "portable fallback"
    );
    Ok(())
}

#[tokio::test]
async fn native_openai_posts_responses_payload_to_responses_endpoint(
) -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    let sse = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"ok\"}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.5\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(sse, "text/event-stream"),
        )
        .mount(&server)
        .await;

    let model = resolved_model_config(
        evotengine::provider::ApiProtocol::OpenAiResponses,
        "openai",
        "gpt-5.5",
        &server.uri(),
        Some(evotengine::provider::OpenAiCompat::openai()),
        evotengine::provider::RouteCapabilities {
            verbosity: true,
            remote_compaction: true,
        },
        Default::default(),
    );
    let config = StreamConfigBuilder::openai()
        .model("gpt-5.5")
        .model_config(model)
        .tools(vec![tool_def("bash", "Run a command")])
        .thinking(ThinkingLevel::Medium)
        .build();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let outcome = OpenAiResponsesProvider
        .stream(config, tx, CancellationToken::new())
        .await?;
    assert!(matches!(outcome.message(), Message::Assistant { .. }));
    let requests = server
        .received_requests()
        .await
        .ok_or("request recording unavailable")?;
    let request = requests.first().ok_or("missing request")?;
    let body: serde_json::Value = serde_json::from_slice(&request.body)?;
    assert_eq!(body["model"], "gpt-5.5");
    assert_eq!(body["reasoning"]["effort"], "medium");
    assert_eq!(body["reasoning"]["summary"], "auto");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["name"], "bash");
    assert!(body.get("messages").is_none());
    assert!(body.get("reasoning_effort").is_none());
    Ok(())
}

#[tokio::test]
async fn responses_streams_reasoning_text_tools_and_usage() -> Result<(), Box<dyn std::error::Error>>
{
    let sse = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.5\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[]}}\n\n",
        "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"delta\":\"Think\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Think\"}],\"encrypted_content\":\"enc\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"Hello\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\",\"annotations\":[]}]}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"command\\\":\\\"pwd\\\"}\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.5\",\"usage\":{\"input_tokens\":100,\"input_tokens_details\":{\"cached_tokens\":25,\"cache_write_tokens\":10},\"output_tokens\":20,\"output_tokens_details\":{\"reasoning_tokens\":5},\"total_tokens\":120}}}\n\n",
    );

    let config = responses_config("gpt-5.5").model("gpt-5.5").build();
    let (message, events) = run_provider_sse(&OpenAiResponsesProvider, config, sse, 200).await?;

    let Message::Assistant {
        content,
        stop_reason,
        usage,
        response_id,
        ..
    } = message
    else {
        return Err("expected assistant message".into());
    };
    assert_eq!(stop_reason, StopReason::ToolUse);
    assert_eq!(response_id.as_deref(), Some("resp_1"));
    assert_eq!(usage.input, 65);
    assert_eq!(usage.cache_read, 25);
    assert_eq!(usage.cache_write, 10);
    assert_eq!(usage.output, 20);
    assert_eq!(usage.reasoning_output, 5);
    assert_eq!(usage.total_tokens, 120);
    assert!(matches!(&content[0], Content::Thinking { thinking, .. } if thinking == "Think"));
    assert!(matches!(&content[1], Content::Text { text } if text == "Hello"));
    assert!(matches!(
        &content[2],
        Content::ToolCall {
            id,
            name,
            arguments,
            metadata: Some(ToolCallMetadata::OpenAiResponses { item_id }),
        }
            if id == "call_1"
                && item_id == "fc_1"
                && name == "bash"
                && arguments["command"] == "pwd"
    ));
    assert!(events.iter().any(
        |event| matches!(event, StreamEvent::ThinkingDelta { delta, .. } if delta == "Think")
    ));
    assert!(events
        .iter()
        .any(|event| matches!(event, StreamEvent::TextDelta { delta, .. } if delta == "Hello")));
    assert!(events
        .iter()
        .any(|event| matches!(event, StreamEvent::ToolCallEnd { id, .. } if id == "call_1")));
    Ok(())
}

#[tokio::test]
async fn responses_accepts_done_only_items_and_backfills_reasoning_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let sse = concat!(
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Think\"}]}}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"refusal\",\"refusal\":\"Cannot comply\"}]}}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.5\",\"output\":[{\"type\":\"reasoning\",\"id\":\"rs_1\",\"encrypted_content\":\"enc\"}],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );

    let (message, _) = run_provider_sse(
        &OpenAiResponsesProvider,
        StreamConfigBuilder::openai().model("gpt-5.5").build(),
        sse,
        200,
    )
    .await?;
    let Message::Assistant { content, .. } = message else {
        return Err("expected assistant message".into());
    };
    assert!(matches!(
        &content[0],
        Content::Thinking {
            thinking,
            metadata: Some(ThinkingMetadata::OpenAiResponses { item }),
        } if thinking == "Think" && item["encrypted_content"] == "enc"
    ));
    assert!(matches!(&content[1], Content::Text { text } if text == "Cannot comply"));
    assert!(matches!(
        &content[2],
        Content::ToolCall {
            id,
            name,
            arguments,
            metadata: Some(ToolCallMetadata::OpenAiResponses { item_id }),
        }
            if id == "call_1"
                && item_id == "fc_1"
                && name == "bash"
                && arguments["command"] == "pwd"
    ));
    Ok(())
}

#[tokio::test]
async fn responses_top_level_error_message_is_preserved() {
    let result = run_provider_sse(
        &OpenAiResponsesProvider,
        responses_config("gpt-4o").build(),
        "event: error\ndata: {\"type\":\"error\",\"code\":\"server_error\",\"message\":\"service unavailable\"}\n\n",
        200,
    )
    .await;
    assert!(result
        .err()
        .is_some_and(|error| error.to_string().contains("service unavailable")));
}

#[tokio::test]
async fn responses_cancelled_is_an_error() {
    let result = run_provider_sse(
        &OpenAiResponsesProvider,
        responses_config("gpt-4o").build(),
        "event: response.cancelled\ndata: {\"type\":\"response.cancelled\",\"response\":{\"id\":\"resp_1\",\"status\":\"cancelled\"}}\n\n",
        200,
    )
    .await;
    assert!(result
        .err()
        .is_some_and(|error| error.to_string().contains("cancelled")));
}

#[tokio::test]
async fn responses_requires_terminal_event() {
    let sse = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"partial\"}\n\n",
    );
    let result = run_provider_sse(
        &OpenAiResponsesProvider,
        responses_config("gpt-4o").build(),
        sse,
        200,
    )
    .await;
    let Err(error) = result else {
        panic!("Expected incomplete protocol error");
    };
    assert!(matches!(
        error,
        evotengine::provider::ProviderError::ProtocolIncomplete(ref message)
            if message.contains("terminal response event")
    ));
    assert!(evotengine::retry::should_retry(&error));
}
