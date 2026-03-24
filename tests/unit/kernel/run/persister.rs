use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use bendclaw::base::ErrorCode;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::run::persist_op::spawn_persist_writer;
use bendclaw::kernel::run::persist_op::PersistOp;
use bendclaw::kernel::run::persist_op::PersistWriter;
use bendclaw::kernel::run::persister::status_from_reason;
use bendclaw::kernel::run::persister::TurnPersister;
use bendclaw::kernel::run::result::ContentBlock;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::run::result::Result as AgentResult;
use bendclaw::kernel::run::result::Usage as AgentUsage;
use bendclaw::kernel::trace::TraceRecorder;
use bendclaw::storage::dal::run::record::RunStatus;
use bendclaw::storage::SpanRepo;
use bendclaw::storage::TraceRepo;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn make_client() -> FakeDatabend {
    FakeDatabend::new(|_sql, _database| Ok(paged_rows(&[], None, None)))
}

fn make_persister(client: &FakeDatabend, writer: &PersistWriter) -> TurnPersister {
    let pool = client.pool();
    let llm = Arc::new(MockLLMProvider::with_text("unused"));
    let storage = Arc::new(AgentStore::new(pool.clone(), llm));
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
        writer.clone(),
    )
}

fn make_result(text: &str, reason: Reason, tokens: u64) -> AgentResult {
    AgentResult {
        content: vec![ContentBlock::text(text)],
        iterations: 1,
        usage: AgentUsage {
            prompt_tokens: tokens,
            completion_tokens: tokens / 2,
            total_tokens: tokens + tokens / 2,
            ..AgentUsage::default()
        },
        stop_reason: reason,
        checkpoint: None,
        messages: Vec::new(),
    }
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

/// Send a Flush barrier and wait — ensures all preceding ops are processed
/// without destroying the writer.
async fn flush(writer: &PersistWriter) {
    let (tx, rx) = tokio::sync::oneshot::channel();
    writer.send(PersistOp::Flush(tx));
    let _ = rx.await;
    // Give trace writer (fire-and-forget) time to flush
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

// ── status_from_reason ───────────────────────────────────────────────────

#[test]
fn status_from_reason_maps_all_variants() {
    assert_eq!(status_from_reason(&Reason::EndTurn), RunStatus::Completed);
    assert_eq!(
        status_from_reason(&Reason::MaxIterations),
        RunStatus::Paused
    );
    assert_eq!(status_from_reason(&Reason::Timeout), RunStatus::Paused);
    assert_eq!(status_from_reason(&Reason::Aborted), RunStatus::Cancelled);
    assert_eq!(status_from_reason(&Reason::Error), RunStatus::Error);
}

// ── persist_success ──────────────────────────────────────────────────────

#[tokio::test]
async fn persist_success_writes_usage_events_run_and_trace() -> Result<()> {
    let client = make_client();
    let writer = spawn_persist_writer();
    let persister = make_persister(&client, &writer);

    let text = persister.persist_success(
        make_result("done", Reason::EndTurn, 10),
        "provider-1",
        "model-1",
        &[Event::Start],
    )?;
    assert_eq!(text, "done");

    flush(&writer).await;

    let sqls = recorded_sqls(&client);
    assert!(sqls.iter().any(|s| s.starts_with("INSERT INTO usage ")));
    assert!(sqls
        .iter()
        .any(|s| s.starts_with("INSERT INTO run_events ") && s.contains("'Start'")));
    assert!(sqls
        .iter()
        .any(|s| { s.starts_with("INSERT INTO run_events ") && s.contains("'run.completed'") }));
    assert!(sqls.iter().any(|s| {
        s.contains("UPDATE runs SET status = 'COMPLETED'") && s.contains("output = 'done'")
    }));
    assert!(sqls
        .iter()
        .any(|s| s.contains("UPDATE traces SET status = 'completed'")));
    Ok(())
}

#[tokio::test]
async fn persist_success_pauses_on_timeout() -> Result<()> {
    let client = make_client();
    let writer = spawn_persist_writer();
    let persister = make_persister(&client, &writer);

    persister.persist_success(make_result("partial", Reason::Timeout, 6), "p", "m", &[
        Event::Start,
    ])?;

    flush(&writer).await;

    let sqls = recorded_sqls(&client);
    assert!(sqls.iter().any(|s| {
        s.contains("UPDATE runs SET status = 'PAUSED'") && s.contains("stop_reason = 'timeout'")
    }));
    assert!(sqls
        .iter()
        .any(|s| s.contains("UPDATE traces SET status = 'completed'")));
    Ok(())
}

#[tokio::test]
async fn persist_success_returns_text_synchronously() {
    let client = make_client();
    let writer = spawn_persist_writer();
    let persister = make_persister(&client, &writer);

    let text = persister
        .persist_success(make_result("fast", Reason::EndTurn, 0), "p", "m", &[])
        .expect("sync call should not fail");
    assert_eq!(text, "fast");
    writer.shutdown().await;
}

// ── persist_error ────────────────────────────────────────────────────────

#[tokio::test]
async fn persist_error_writes_events_run_and_trace() {
    let client = make_client();
    let writer = spawn_persist_writer();
    let persister = make_persister(&client, &writer);

    persister.persist_error(&ErrorCode::internal("boom"), &[Event::Start]);

    flush(&writer).await;

    let sqls = recorded_sqls(&client);
    assert!(sqls
        .iter()
        .any(|s| s.starts_with("INSERT INTO run_events ") && s.contains("'run.failed'")));
    assert!(sqls
        .iter()
        .any(|s| s.contains("UPDATE runs SET status = 'ERROR'") && s.contains("boom")));
    assert!(sqls
        .iter()
        .any(|s| s.contains("UPDATE traces SET status = 'failed'")));
}

// ── persist_cancelled ────────────────────────────────────────────────────

#[tokio::test]
async fn persist_cancelled_writes_events_run_and_trace() {
    let client = make_client();
    let writer = spawn_persist_writer();
    let persister = make_persister(&client, &writer);

    persister.persist_cancelled(&[Event::Start]);

    flush(&writer).await;

    let sqls = recorded_sqls(&client);
    assert!(sqls
        .iter()
        .any(|s| { s.starts_with("INSERT INTO run_events ") && s.contains("'run.cancelled'") }));
    assert!(sqls.iter().any(
        |s| s == "UPDATE runs SET status = 'CANCELLED', updated_at = NOW() WHERE id = 'run-1'"
    ));
    assert!(sqls
        .iter()
        .any(|s| s.contains("UPDATE traces SET status = 'failed'")));
}
