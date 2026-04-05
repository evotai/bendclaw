use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::conf::LlmConfig;
use bendclaw::conf::ProviderKind;
use bendclaw::conf::StorageConfig;
use bendclaw::error::Result;
use bendclaw::request::*;
use bendclaw::storage::model::ListRunEvents;
use bendclaw::storage::model::ListTranscriptEntries;
use bendclaw::storage::model::RunEvent;
use bendclaw::storage::model::RunEventKind;
use bendclaw::storage::model::RunMeta;
use bendclaw::storage::model::RunStatus;
use bendclaw::storage::open_storage;
use tempfile::TempDir;
use tokio::sync::Mutex;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn is_uuid_v7(value: &str) -> bool {
    match uuid::Uuid::parse_str(value) {
        Ok(value) => value.get_version_num() == 7,
        Err(_) => false,
    }
}

fn fs_store(root: &TempDir) -> StorageConfig {
    StorageConfig::fs(root.path().to_path_buf())
}

fn test_llm_config() -> LlmConfig {
    LlmConfig {
        provider: ProviderKind::Anthropic,
        api_key: "test-key".into(),
        base_url: None,
        model: "claude-sonnet-4-20250514".into(),
    }
}

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

struct CollectSink {
    events: Mutex<Vec<Arc<RunEvent>>>,
}

impl CollectSink {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    async fn events(&self) -> Vec<Arc<RunEvent>> {
        self.events.lock().await.clone()
    }
}

#[async_trait]
impl EventSink for CollectSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn full_pipeline_creates_session_and_run() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;
    let sink = Arc::new(CollectSink::new());

    let final_messages = vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hi there".into(),
            }],
        },
    ];

    let sdk_messages = vec![
        bend_agent::SDKMessage::System {
            message: "started".into(),
        },
        bend_agent::SDKMessage::Assistant {
            message: bend_agent::Message {
                role: bend_agent::MessageRole::Assistant,
                content: vec![bend_agent::ContentBlock::Text {
                    text: "hi there".into(),
                }],
            },
            usage: None,
        },
        bend_agent::SDKMessage::Result {
            text: "hi there".into(),
            usage: bend_agent::Usage::default(),
            num_turns: 1,
            cost_usd: 0.001,
            duration_ms: 100,
            messages: final_messages.clone(),
        },
    ];

    let runner = RequestRunner::scripted(sdk_messages, final_messages);
    let request = Request::new("hello".into());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink.clone(),
        storage.clone(),
        runner,
    )
    .execute()
    .await?;

    let events = sink.events().await;
    assert!(events.len() >= 4);

    let kinds: Vec<_> = events.iter().map(|event| &event.kind).collect();
    assert!(matches!(kinds[0], RunEventKind::RunStarted));
    assert!(matches!(kinds[1], RunEventKind::System));
    assert!(matches!(kinds[2], RunEventKind::AssistantMessage));
    assert!(matches!(kinds[3], RunEventKind::RunFinished));

    let session_id = &events[0].session_id;
    let run_id = &events[0].run_id;

    assert!(is_uuid_v7(session_id));
    assert!(is_uuid_v7(run_id));
    assert!(is_uuid_v7(&events[0].event_id));

    let session_meta = storage
        .get_session(session_id)
        .await?
        .ok_or_else(|| missing_error("missing session meta"))?;
    assert_eq!(session_meta.session_id, *session_id);

    let transcript = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: session_id.clone(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(transcript.len(), 2);

    let run_events = storage
        .list_run_events(ListRunEvents {
            run_id: run_id.clone(),
        })
        .await?;
    assert_eq!(run_events.len(), 4);

    Ok(())
}

#[tokio::test]
async fn pipeline_marks_failed_when_no_result() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;
    let sink = Arc::new(CollectSink::new());

    let sdk_messages = vec![bend_agent::SDKMessage::Error {
        message: "api failed".into(),
    }];

    let runner = RequestRunner::scripted(sdk_messages, vec![]);
    let request = Request::new("hello".into());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink.clone(),
        storage.clone(),
        runner,
    )
    .execute()
    .await?;

    let events = sink.events().await;
    let run_id = &events[0].run_id;
    let session_id = &events[0].session_id;
    let meta_path = root
        .path()
        .join("sessions")
        .join(session_id)
        .join("runs")
        .join(format!("{run_id}.json"));
    let content = std::fs::read_to_string(meta_path)?;
    let run_meta: RunMeta = serde_json::from_str(&content)?;
    assert_eq!(run_meta.status, RunStatus::Failed);

    Ok(())
}

#[tokio::test]
async fn pipeline_resume_session() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;

    let first_messages = vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
        },
    ];

    let first_sdk = vec![
        bend_agent::SDKMessage::Assistant {
            message: first_messages[1].clone(),
            usage: None,
        },
        bend_agent::SDKMessage::Result {
            text: "hi".into(),
            usage: bend_agent::Usage::default(),
            num_turns: 1,
            cost_usd: 0.0,
            duration_ms: 50,
            messages: first_messages.clone(),
        },
    ];

    let runner1 = RequestRunner::scripted(first_sdk, first_messages.clone());
    let sink1 = Arc::new(CollectSink::new());

    RequestExecutor::new(
        Request::new("hello".into()),
        test_llm_config(),
        sink1.clone(),
        storage.clone(),
        runner1,
    )
    .execute()
    .await?;

    let session_id = sink1
        .events()
        .await
        .first()
        .ok_or_else(|| missing_error("missing first event"))?
        .session_id
        .clone();

    let second_messages = vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "continue".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text { text: "ok".into() }],
        },
    ];

    let second_sdk = vec![
        bend_agent::SDKMessage::Assistant {
            message: second_messages[3].clone(),
            usage: None,
        },
        bend_agent::SDKMessage::Result {
            text: "ok".into(),
            usage: bend_agent::Usage::default(),
            num_turns: 1,
            cost_usd: 0.0,
            duration_ms: 50,
            messages: second_messages.clone(),
        },
    ];

    let runner2 = RequestRunner::scripted(second_sdk, second_messages.clone());
    let sink2 = Arc::new(CollectSink::new());
    let mut request = Request::new("continue".into());
    request.session_id = Some(session_id.clone());

    RequestExecutor::new(
        request,
        test_llm_config(),
        sink2.clone(),
        storage.clone(),
        runner2,
    )
    .execute()
    .await?;

    let transcript = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: session_id.clone(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(transcript.len(), 4);

    Ok(())
}

#[test]
fn map_all_sdk_message_variants() {
    let run_id = "run-001";
    let session_id = "sess-001";

    let cases: Vec<(bend_agent::SDKMessage, RunEventKind)> = vec![
        (
            bend_agent::SDKMessage::System {
                message: "started".into(),
            },
            RunEventKind::System,
        ),
        (
            bend_agent::SDKMessage::Assistant {
                message: bend_agent::Message {
                    role: bend_agent::MessageRole::Assistant,
                    content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
                },
                usage: None,
            },
            RunEventKind::AssistantMessage,
        ),
        (
            bend_agent::SDKMessage::ToolResult {
                tool_use_id: "t1".into(),
                tool_name: "Read".into(),
                content: "ok".into(),
                is_error: false,
            },
            RunEventKind::ToolResult,
        ),
        (
            bend_agent::SDKMessage::PartialMessage {
                text: "partial".into(),
            },
            RunEventKind::PartialMessage,
        ),
        (
            bend_agent::SDKMessage::CompactBoundary {
                summary: "compacted".into(),
            },
            RunEventKind::CompactBoundary,
        ),
        (
            bend_agent::SDKMessage::Status {
                message: "ok".into(),
            },
            RunEventKind::Status,
        ),
        (
            bend_agent::SDKMessage::TaskNotification {
                task_id: "task-1".into(),
                status: "done".into(),
                message: None,
            },
            RunEventKind::TaskNotification,
        ),
        (
            bend_agent::SDKMessage::RateLimit {
                retry_after_ms: 1000,
                message: "slow down".into(),
            },
            RunEventKind::RateLimit,
        ),
        (
            bend_agent::SDKMessage::Progress {
                message: "50%".into(),
            },
            RunEventKind::Progress,
        ),
        (
            bend_agent::SDKMessage::Error {
                message: "fail".into(),
            },
            RunEventKind::Error,
        ),
        (
            bend_agent::SDKMessage::Result {
                text: "done".into(),
                usage: bend_agent::Usage::default(),
                num_turns: 1,
                cost_usd: 0.01,
                duration_ms: 100,
                messages: vec![],
            },
            RunEventKind::RunFinished,
        ),
    ];

    for (message, expected_kind) in cases {
        let event = map_sdk_message(&message, run_id, session_id, 1);
        assert_eq!(event.run_id, run_id);
        assert_eq!(event.session_id, session_id);
        assert_eq!(
            std::mem::discriminant(&event.kind),
            std::mem::discriminant(&expected_kind),
        );
    }
}

#[test]
fn request_started_event_has_correct_kind() {
    let event = request_started_event("run-001", "sess-001");
    assert!(matches!(event.kind, RunEventKind::RunStarted));
    assert_eq!(event.turn, 0);
}

#[test]
fn assistant_event_payload_is_typed() -> TestResult {
    let message = bend_agent::SDKMessage::Assistant {
        message: bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![
                bend_agent::ContentBlock::Text { text: "hi".into() },
                bend_agent::ContentBlock::ToolUse {
                    id: "tool-1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({ "path": "a.txt" }),
                },
            ],
        },
        usage: None,
    };

    let event = map_sdk_message(&message, "run-001", "sess-001", 1);
    let payload = payload_as::<AssistantPayload>(&event.payload)
        .ok_or_else(|| missing_error("missing assistant payload"))?;
    assert_eq!(payload.role, "assistant");
    assert_eq!(payload.content.len(), 2);
    assert!(matches!(payload.content[0], AssistantBlock::Text { .. }));
    assert!(matches!(payload.content[1], AssistantBlock::ToolUse { .. }));
    Ok(())
}

#[test]
fn message_event_payload_is_typed() -> TestResult {
    let message = bend_agent::SDKMessage::Progress {
        message: "working".into(),
    };
    let event = map_sdk_message(&message, "run-001", "sess-001", 1);
    let payload = payload_as::<MessagePayload>(&event.payload)
        .ok_or_else(|| missing_error("missing message payload"))?;
    assert_eq!(payload.message, "working");
    Ok(())
}

#[test]
fn tool_result_event_payload_is_typed() -> TestResult {
    let message = bend_agent::SDKMessage::ToolResult {
        tool_use_id: "tool-1".into(),
        tool_name: "Read".into(),
        content: "done".into(),
        is_error: false,
    };
    let event = map_sdk_message(&message, "run-001", "sess-001", 1);
    let payload = payload_as::<ToolResultPayload>(&event.payload)
        .ok_or_else(|| missing_error("missing tool result payload"))?;
    assert_eq!(payload.tool_name, "Read");
    assert_eq!(payload.content, "done");
    assert!(!payload.is_error);
    Ok(())
}
