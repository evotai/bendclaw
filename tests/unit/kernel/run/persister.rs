use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::persister::status_from_reason;
use bendclaw::kernel::run::persister::TurnPersister;
use bendclaw::kernel::run::result::ContentBlock;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::run::result::Result as AgentResult;
use bendclaw::kernel::run::result::Usage as AgentUsage;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::storage::dal::run::record::RunStatus;
use bendclaw::storage::SpanRepo;
use bendclaw::storage::TraceRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

struct PricingLLM;

#[async_trait]
impl LLMProvider for PricingLLM {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        Err(ErrorCode::internal("not used in persister tests"))
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let (_writer, stream) = ResponseStream::channel(1);
        stream
    }

    fn pricing(&self, _model: &str) -> Option<(f64, f64)> {
        Some((1.0, 2.0))
    }

    fn default_model(&self) -> &str {
        "mock"
    }

    fn default_temperature(&self) -> f64 {
        0.0
    }
}

fn make_client() -> FakeDatabend {
    FakeDatabend::new(|_sql, _database| Ok(paged_rows(&[], None, None)))
}

fn make_persister(client: &FakeDatabend) -> TurnPersister {
    let pool = client.pool();
    let storage = Arc::new(AgentStore::new(pool.clone(), Arc::new(PricingLLM)));
    let trace = TraceRecorder::new(
        Arc::new(TraceRepo::new(pool.clone())),
        Arc::new(SpanRepo::new(pool)),
        "trace-1",
        "run-1",
        "agent-1",
        "session-1",
        "user-1",
    );
    TurnPersister::new(
        storage,
        trace,
        Arc::<str>::from("agent-1"),
        "session-1",
        "run-1",
        Arc::<str>::from("user-1"),
        Instant::now(),
        None,
    )
}

fn recorded_sqls(client: &FakeDatabend) -> Vec<String> {
    client
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            FakeDatabendCall::Query { sql, .. } => Some(sql),
            _ => None,
        })
        .collect()
}

#[test]
fn status_from_reason_maps_terminal_run_status() {
    assert_eq!(status_from_reason(&Reason::EndTurn), RunStatus::Completed);
    assert_eq!(
        status_from_reason(&Reason::MaxIterations),
        RunStatus::Paused
    );
    assert_eq!(status_from_reason(&Reason::Timeout), RunStatus::Paused);
    assert_eq!(status_from_reason(&Reason::Aborted), RunStatus::Cancelled);
    assert_eq!(status_from_reason(&Reason::Error), RunStatus::Error);
}

#[tokio::test]
async fn persist_success_updates_run_events_usage_and_trace() -> Result<()> {
    let client = make_client();
    let persister = make_persister(&client);
    let result = AgentResult {
        content: vec![ContentBlock::text("done")],
        iterations: 3,
        usage: AgentUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            reasoning_tokens: 2,
            total_tokens: 15,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            ttft_ms: 7,
        },
        stop_reason: Reason::EndTurn,
        messages: Vec::new(),
    };

    let text = persister
        .persist_success(result, "mock-provider", "mock-model", &[Event::Start])
        .await?;

    assert_eq!(text, "done");
    let sqls = recorded_sqls(&client);
    assert!(sqls.iter().any(|sql| sql.starts_with("INSERT INTO usage ")));
    assert!(sqls
        .iter()
        .any(|sql| sql.starts_with("INSERT INTO run_events ") && sql.contains("'Start'")));
    assert!(sqls.iter().any(|sql| {
        sql.starts_with("INSERT INTO run_events ") && sql.contains("'run.completed'")
    }));
    assert!(sqls.iter().any(|sql| {
        sql.contains("UPDATE runs SET status = 'COMPLETED'") && sql.contains("output = 'done'")
    }));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'completed'")));
    Ok(())
}

#[tokio::test]
async fn persist_success_pauses_run_for_timeout_reason() -> Result<()> {
    let client = make_client();
    let persister = make_persister(&client);
    let result = AgentResult {
        content: vec![ContentBlock::text("partial")],
        iterations: 5,
        usage: AgentUsage {
            prompt_tokens: 3,
            completion_tokens: 2,
            reasoning_tokens: 0,
            total_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            ttft_ms: 0,
        },
        stop_reason: Reason::Timeout,
        messages: Vec::new(),
    };

    let _ = persister
        .persist_success(result, "mock-provider", "mock-model", &[Event::Start])
        .await?;

    let sqls = recorded_sqls(&client);
    assert!(sqls.iter().any(|sql| {
        sql.contains("UPDATE runs SET status = 'PAUSED'") && sql.contains("stop_reason = 'timeout'")
    }));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'completed'")));
    Ok(())
}

#[tokio::test]
async fn persist_error_marks_run_failed_and_persists_failure_event() {
    let client = make_client();
    let persister = make_persister(&client);

    persister
        .persist_error(&ErrorCode::internal("boom"), &[Event::Start])
        .await;

    let sqls = recorded_sqls(&client);
    assert!(sqls
        .iter()
        .any(|sql| { sql.starts_with("INSERT INTO run_events ") && sql.contains("'run.failed'") }));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE runs SET status = 'ERROR'") && sql.contains("boom")));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'failed'")));
}

#[tokio::test]
async fn persist_cancelled_marks_run_cancelled_and_persists_cancel_event() {
    let client = make_client();
    let persister = make_persister(&client);

    persister.persist_cancelled(&[Event::Start]).await;

    let sqls = recorded_sqls(&client);
    assert!(sqls.iter().any(|sql| {
        sql.starts_with("INSERT INTO run_events ") && sql.contains("'run.cancelled'")
    }));
    assert!(sqls
        .iter()
        .any(|sql| sql
            == "UPDATE runs SET status = 'CANCELLED', updated_at = NOW() WHERE id = 'run-1'"));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'failed'")));
}
