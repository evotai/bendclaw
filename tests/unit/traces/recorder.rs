use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use bendclaw::storage::pool::DatabendClient;
use bendclaw::storage::pool::QueryResponse;
use bendclaw::storage::Pool;
use bendclaw::storage::SpanRepo;
use bendclaw::storage::TraceRepo;
use bendclaw::traces::recorder::Trace;
use bendclaw::traces::recorder::TraceRecorder;

#[derive(Clone, Default)]
struct RecordingClient {
    sqls: Arc<Mutex<Vec<String>>>,
}

impl RecordingClient {
    fn sqls(&self) -> Vec<String> {
        self.sqls.lock().expect("trace sqls lock").clone()
    }
}

#[async_trait]
impl DatabendClient for RecordingClient {
    async fn query(
        &self,
        sql: &str,
        _database: Option<&str>,
    ) -> bendclaw::types::Result<QueryResponse> {
        self.sqls
            .lock()
            .expect("trace sqls lock")
            .push(sql.to_string());
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    }

    async fn page(&self, _uri: &str) -> bendclaw::types::Result<QueryResponse> {
        unreachable!("trace recorder should not request pages")
    }

    async fn finalize(&self, _uri: &str) -> bendclaw::types::Result<()> {
        Ok(())
    }
}

fn fake_pool(client: &RecordingClient) -> Pool {
    Pool::from_client("http://fake.local/v1", "default", Arc::new(client.clone()))
}

#[tokio::test]
async fn trace_recorder_persists_trace_and_completed_span() {
    let client = RecordingClient::default();
    let pool = fake_pool(&client);
    let recorder = TraceRecorder::new(
        Arc::new(TraceRepo::new(pool.clone())),
        Arc::new(SpanRepo::new(pool)),
        "trace-1",
        "run-1",
        "agent-1",
        "session-1",
        "user-1",
    );

    recorder.start_trace("agent.run");
    let trace = Trace::new(recorder.clone());
    let span = trace.start_span("tool", "shell", "", "assistant", "{}", "echo hi");
    span.complete(12, 3, 4, 5, 0, 0.25, "{}", "done").await;
    recorder.complete_trace(42, 10, 20, 0.5);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let sqls = client.sqls();
    assert!(sqls.iter().any(|sql| sql.contains("INSERT INTO traces")));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'started'")));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'completed'")));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'completed'")));
}

#[tokio::test]
async fn trace_recorder_persists_failed_and_cancelled_spans() {
    let client = RecordingClient::default();
    let pool = fake_pool(&client);
    let recorder = TraceRecorder::new(
        Arc::new(TraceRepo::new(pool.clone())),
        Arc::new(SpanRepo::new(pool)),
        "trace-2",
        "run-2",
        "agent-2",
        "session-2",
        "user-2",
    );

    let trace = Trace::new(recorder.clone());
    let span = trace.start_span("skill", "remote-tool", "", "assistant", "{}", "run skill");
    span.fail(9, "oops", "failed to run", "{}", "broken").await;
    recorder.cancelled_span("span-cancelled", "", "tool", "shell", 7, "cancelled");
    recorder.fail_trace(99);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let sqls = client.sqls();
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'failed'")));
    assert!(sqls.iter().any(|sql| {
        sql.contains("INSERT INTO spans")
            && sql.contains("'cancelled'")
            && sql.contains("operation cancelled")
    }));
    assert!(sqls
        .iter()
        .any(|sql| sql.contains("UPDATE traces SET status = 'failed'")));
}
