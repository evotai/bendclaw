use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::env::LlmConfig;
use bendclaw::env::ProviderKind;
use bendclaw::error::Result;
use bendclaw::run::*;
use bendclaw::store::create_stores;
use bendclaw::store::StoreBackend;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

fn test_llm_config() -> LlmConfig {
    LlmConfig {
        provider: ProviderKind::Anthropic,
        api_key: "test-key".into(),
        base_url: None,
        model: "claude-sonnet-4-20250514".into(),
    }
}

struct MockRunner {
    messages_to_send: Vec<bend_agent::SDKMessage>,
    final_messages: Vec<bend_agent::Message>,
    closed: Mutex<bool>,
}

impl MockRunner {
    fn new(
        messages_to_send: Vec<bend_agent::SDKMessage>,
        final_messages: Vec<bend_agent::Message>,
    ) -> Self {
        Self {
            messages_to_send,
            final_messages,
            closed: Mutex::new(false),
        }
    }
}

#[async_trait]
impl AgentRunner for MockRunner {
    async fn run_query(
        &self,
        _options: AgentRunOptions,
    ) -> Result<mpsc::Receiver<bend_agent::SDKMessage>> {
        let (tx, rx) = mpsc::channel(100);
        let msgs = self.messages_to_send.clone();
        tokio::spawn(async move {
            for msg in msgs {
                let _ = tx.send(msg).await;
            }
        });
        Ok(rx)
    }

    async fn take_messages(&self) -> Vec<bend_agent::Message> {
        self.final_messages.clone()
    }

    async fn close(&self) {
        *self.closed.lock().await = true;
    }
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
async fn full_pipeline_creates_session_and_run() {
    let sessions_dir = TempDir::new().unwrap();
    let runs_dir = TempDir::new().unwrap();

    let stores = create_stores(StoreBackend::Fs {
        session_dir: sessions_dir.path().to_path_buf(),
        run_dir: runs_dir.path().to_path_buf(),
    })
    .unwrap();
    let sink = CollectSink::new();

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

    let runner = MockRunner::new(sdk_messages, final_messages);
    let request = RunRequest::new("hello".into());

    run_with_runner(
        request,
        test_llm_config(),
        &sink,
        stores.session.as_ref(),
        stores.run.as_ref(),
        &runner,
    )
    .await
    .unwrap();

    let events = sink.events().await;
    assert!(events.len() >= 4);

    let kinds: Vec<_> = events.iter().map(|e| &e.kind).collect();
    assert!(matches!(kinds[0], RunEventKind::RunStarted));
    assert!(matches!(kinds[1], RunEventKind::System));
    assert!(matches!(kinds[2], RunEventKind::AssistantMessage));
    assert!(matches!(kinds[3], RunEventKind::RunFinished));

    let session_id = &events[0].session_id;
    let run_id = &events[0].run_id;

    let meta = stores.session.load_meta(session_id).await.unwrap();
    assert!(meta.is_some());

    let transcript = stores.session.load_transcript(session_id).await.unwrap();
    assert!(transcript.is_some());
    assert_eq!(transcript.unwrap().len(), 2);

    let run_events = stores.run.load_events(run_id).await.unwrap();
    assert_eq!(run_events.len(), 4);
}

#[tokio::test]
async fn pipeline_marks_failed_when_no_result() {
    let sessions_dir = TempDir::new().unwrap();
    let runs_dir = TempDir::new().unwrap();

    let stores = create_stores(StoreBackend::Fs {
        session_dir: sessions_dir.path().to_path_buf(),
        run_dir: runs_dir.path().to_path_buf(),
    })
    .unwrap();
    let sink = CollectSink::new();

    let sdk_messages = vec![bend_agent::SDKMessage::Error {
        message: "api failed".into(),
    }];

    let runner = MockRunner::new(sdk_messages, vec![]);
    let request = RunRequest::new("hello".into());

    run_with_runner(
        request,
        test_llm_config(),
        &sink,
        stores.session.as_ref(),
        stores.run.as_ref(),
        &runner,
    )
    .await
    .unwrap();

    let events = sink.events().await;
    let run_id = &events[0].run_id;

    let meta_path = runs_dir.path().join(format!("{run_id}.json"));
    let content = std::fs::read_to_string(meta_path).unwrap();
    let run_meta: bendclaw::run::RunMeta = serde_json::from_str(&content).unwrap();
    assert_eq!(run_meta.status, bendclaw::run::RunStatus::Failed);
}

#[tokio::test]
async fn pipeline_resume_session() {
    let sessions_dir = TempDir::new().unwrap();
    let runs_dir = TempDir::new().unwrap();

    let stores = create_stores(StoreBackend::Fs {
        session_dir: sessions_dir.path().to_path_buf(),
        run_dir: runs_dir.path().to_path_buf(),
    })
    .unwrap();

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

    let runner1 = MockRunner::new(first_sdk, first_messages.clone());
    let sink1 = CollectSink::new();
    let req1 = RunRequest::new("hello".into());

    run_with_runner(
        req1,
        test_llm_config(),
        &sink1,
        stores.session.as_ref(),
        stores.run.as_ref(),
        &runner1,
    )
    .await
    .unwrap();

    let session_id = sink1.events().await[0].session_id.clone();

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

    let runner2 = MockRunner::new(second_sdk, second_messages.clone());
    let sink2 = CollectSink::new();
    let mut req2 = RunRequest::new("continue".into());
    req2.session_id = Some(session_id.clone());

    run_with_runner(
        req2,
        test_llm_config(),
        &sink2,
        stores.session.as_ref(),
        stores.run.as_ref(),
        &runner2,
    )
    .await
    .unwrap();

    let transcript = stores
        .session
        .load_transcript(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(transcript.len(), 4);
}
