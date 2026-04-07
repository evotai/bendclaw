use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::agent::AppAgent;
use bendclaw::cli::app::open_session;
use bendclaw::cli::app::run_prompt;
use bendclaw::cli::app::EventSink;
use bendclaw::conf::StorageConfig;
use bendclaw::error::Result;
use bendclaw::protocol::*;
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

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

fn make_assistant_transcript(text: &str) -> TranscriptItem {
    TranscriptItem::Assistant {
        text: text.into(),
        thinking: None,
        tool_calls: vec![],
        stop_reason: "stop".into(),
    }
}
fn make_assistant_completed_event(text: &str) -> ProtocolEvent {
    ProtocolEvent::AssistantCompleted {
        content: vec![AssistantBlock::Text { text: text.into() }],
        usage: Some(UsageSummary {
            input: 10,
            output: 5,
        }),
        stop_reason: "stop".into(),
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
async fn full_pipeline_creates_session_and_run() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&fs_store(&root))?;
    let sink = Arc::new(CollectSink::new());

    let final_transcripts = vec![
        TranscriptItem::User {
            text: "hello".into(),
        },
        make_assistant_transcript("hi there"),
    ];

    let agent_events = vec![
        ProtocolEvent::TurnStart,
        make_assistant_completed_event("hi there"),
        ProtocolEvent::AgentEnd {
            transcripts: final_transcripts.clone(),
            usage: UsageSummary {
                input: 10,
                output: 5,
            },
            transcript_count: 2,
        },
    ];

    let agent = AppAgent::scripted(agent_events, final_transcripts);

    let session = open_session(None, &storage, "/tmp", "").await?;
    run_prompt(
        agent,
        "hello".into(),
        session,
        sink.clone(),
        storage.clone(),
    )
    .await?;

    let events = sink.events().await;
    assert!(events.len() >= 4);

    let kinds: Vec<_> = events.iter().map(|event| event.kind_str()).collect();
    assert_eq!(kinds[0], "run_started");
    assert_eq!(kinds[1], "turn_started");
    assert_eq!(kinds[2], "assistant_completed");
    assert_eq!(kinds[3], "run_finished");

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

    let agent_events = vec![ProtocolEvent::InputRejected {
        reason: "api failed".into(),
    }];

    let agent = AppAgent::scripted(agent_events, vec![]);

    let session = open_session(None, &storage, "/tmp", "").await?;
    run_prompt(
        agent,
        "hello".into(),
        session,
        sink.clone(),
        storage.clone(),
    )
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

    let first_transcripts = vec![
        TranscriptItem::User {
            text: "hello".into(),
        },
        make_assistant_transcript("hi"),
    ];

    let first_events = vec![
        ProtocolEvent::TurnStart,
        make_assistant_completed_event("hi"),
        ProtocolEvent::AgentEnd {
            transcripts: first_transcripts.clone(),
            usage: UsageSummary {
                input: 10,
                output: 5,
            },
            transcript_count: 2,
        },
    ];

    let agent1 = AppAgent::scripted(first_events, first_transcripts);
    let sink1 = Arc::new(CollectSink::new());

    let session1 = open_session(None, &storage, "/tmp", "").await?;
    run_prompt(
        agent1,
        "hello".into(),
        session1,
        sink1.clone(),
        storage.clone(),
    )
    .await?;

    let session_id = sink1
        .events()
        .await
        .first()
        .ok_or_else(|| missing_error("missing first event"))?
        .session_id
        .clone();

    let second_transcripts = vec![
        TranscriptItem::User {
            text: "hello".into(),
        },
        make_assistant_transcript("hi"),
        TranscriptItem::User {
            text: "continue".into(),
        },
        make_assistant_transcript("ok"),
    ];

    let second_events = vec![
        ProtocolEvent::TurnStart,
        make_assistant_completed_event("ok"),
        ProtocolEvent::AgentEnd {
            transcripts: second_transcripts.clone(),
            usage: UsageSummary {
                input: 20,
                output: 10,
            },
            transcript_count: 4,
        },
    ];

    let agent2 = AppAgent::scripted(second_events, second_transcripts);
    let sink2 = Arc::new(CollectSink::new());

    let session2 = open_session(Some(&session_id), &storage, "/tmp", "").await?;
    run_prompt(
        agent2,
        "continue".into(),
        session2,
        sink2.clone(),
        storage.clone(),
    )
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

    let kinds: Vec<_> = transcript.iter().map(|e| &e.kind).collect();
    assert!(matches!(kinds[0], TranscriptKind::User));
    assert!(matches!(kinds[1], TranscriptKind::Assistant));
    assert!(matches!(kinds[2], TranscriptKind::User));
    assert!(matches!(kinds[3], TranscriptKind::Assistant));

    Ok(())
}

#[test]
fn request_started_event_has_correct_kind() {
    let event = RunEventContext::new("run-001", "sess-001", 0).started();
    assert_eq!(event.kind_str(), "run_started");
    assert_eq!(event.turn, 0);
}
