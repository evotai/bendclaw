use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::tools::OperationMeta;
use bendclaw::kernel::OpType;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[derive(Clone, Default)]
struct RunExecState {
    run: Arc<Mutex<Option<StoredRun>>>,
}

#[derive(Clone)]
struct StoredRun {
    id: String,
    session_id: String,
    status: String,
    input: String,
    output: String,
    error: String,
    metrics: String,
    stop_reason: String,
    iterations: u32,
}

fn quoted_values(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(next) = chars.next() {
            if next == '\'' {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                    continue;
                }
                break;
            }
            value.push(next);
        }
        out.push(value);
    }
    out
}

fn run_row(run_id: &str, status: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String(run_id.to_string()),
            serde_json::Value::String("session-1".to_string()),
            serde_json::Value::String("agent-1".to_string()),
            serde_json::Value::String("user-1".to_string()),
            serde_json::Value::String("user_turn".to_string()),
            serde_json::Value::String(String::new()), // parent_run_id
            serde_json::Value::String(String::new()), // node_id
            serde_json::Value::String(status.to_string()),
            serde_json::Value::String("hello".to_string()),
            serde_json::Value::String("done".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("{\"duration_ms\":42}".to_string()),
            serde_json::Value::String("END_TURN".to_string()),
            serde_json::Value::String(String::new()), // checkpoint_through_run_id
            serde_json::Value::String("3".to_string()),
            serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-11T00:01:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn stored_run_row(run: &StoredRun) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String(run.id.clone()),
            serde_json::Value::String(run.session_id.clone()),
            serde_json::Value::String("agent-1".to_string()),
            serde_json::Value::String("user-1".to_string()),
            serde_json::Value::String("user_turn".to_string()),
            serde_json::Value::String(String::new()), // parent_run_id
            serde_json::Value::String(String::new()), // node_id
            serde_json::Value::String(run.status.clone()),
            serde_json::Value::String(run.input.clone()),
            serde_json::Value::String(run.output.clone()),
            serde_json::Value::String(run.error.clone()),
            serde_json::Value::String(run.metrics.clone()),
            serde_json::Value::String(run.stop_reason.clone()),
            serde_json::Value::String(String::new()), // checkpoint_through_run_id
            serde_json::Value::String(run.iterations.to_string()),
            serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-11T00:01:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

fn run_event_rows(run_id: &str) -> bendclaw::storage::pool::QueryResponse {
    let start = serde_json::to_string(&Event::Start).expect("serialize start event");
    let skipped =
        serde_json::to_string(&Event::TurnStart { iteration: 1 }).expect("serialize turn start");
    let tool_end = serde_json::to_string(&Event::ToolEnd {
        tool_call_id: "call-1".to_string(),
        name: "shell".to_string(),
        success: true,
        output: "hello".to_string(),
        operation: OperationMeta {
            op_type: OpType::Execute,
            impact: None,
            timeout_secs: None,
            duration_ms: 12,
            summary: "echo hello".to_string(),
        },
    })
    .expect("serialize tool end");

    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![
            vec![
                serde_json::Value::String("evt-1".to_string()),
                serde_json::Value::String(run_id.to_string()),
                serde_json::Value::String("session-1".to_string()),
                serde_json::Value::String("agent-1".to_string()),
                serde_json::Value::String("user-1".to_string()),
                serde_json::Value::String("1".to_string()),
                serde_json::Value::String("Start".to_string()),
                serde_json::Value::String(start),
                serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            ],
            vec![
                serde_json::Value::String("evt-2".to_string()),
                serde_json::Value::String(run_id.to_string()),
                serde_json::Value::String("session-1".to_string()),
                serde_json::Value::String("agent-1".to_string()),
                serde_json::Value::String("user-1".to_string()),
                serde_json::Value::String("2".to_string()),
                serde_json::Value::String("TurnStart".to_string()),
                serde_json::Value::String(skipped),
                serde_json::Value::String("2026-03-11T00:00:01Z".to_string()),
            ],
            vec![
                serde_json::Value::String("evt-3".to_string()),
                serde_json::Value::String(run_id.to_string()),
                serde_json::Value::String("session-1".to_string()),
                serde_json::Value::String("agent-1".to_string()),
                serde_json::Value::String("user-1".to_string()),
                serde_json::Value::String("3".to_string()),
                serde_json::Value::String("ToolEnd".to_string()),
                serde_json::Value::String(tool_end),
                serde_json::Value::String("2026-03-11T00:00:02Z".to_string()),
            ],
        ],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

async fn fake_runs_app() -> Result<axum::Router> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("SHOW DATABASES LIKE ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT COUNT(*) FROM runs WHERE session_id = 'session-1'") {
            return Ok(paged_rows(&[&["1"]], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE session_id = 'session-1' AND kind != 'session_checkpoint'") {
            return Ok(run_row("run-1", "COMPLETED"));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE id = 'run-1' LIMIT 1") {
            return Ok(run_row("run-1", "COMPLETED"));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE id = 'run-paused' LIMIT 1") {
            return Ok(run_row("run-paused", "COMPLETED"));
        }
        if sql.starts_with("SELECT id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at) FROM run_events WHERE run_id = 'run-1' ORDER BY seq ASC, created_at ASC LIMIT 5000") {
            return Ok(run_event_rows("run-1"));
        }
        panic!("unexpected SQL in runs fast test: {sql}");
    });
    let prefix = format!(
        "test_fast_run_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await
}

async fn fake_execute_runs_app(state: RunExecState) -> Result<axum::Router> {
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("SHOW DATABASES LIKE ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT agent_id, system_prompt, display_name, description, identity, soul, token_limit_total, token_limit_daily, llm_config, created_by, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM agent_config WHERE agent_id = ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, key, value, secret, revoked, user_id, scope, created_by, TO_VARCHAR(last_used_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM variables WHERE revoked = FALSE") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, agent_id, user_id, title, scope, base_key, replaced_by_session_id, reset_reason, PARSE_JSON(session_state), PARSE_JSON(meta), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM sessions WHERE id = ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("REPLACE INTO sessions ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE session_id = 'session-1' AND kind = 'session_checkpoint' ORDER BY created_at DESC LIMIT 1") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE session_id = 'session-1' AND kind != 'session_checkpoint' ORDER BY created_at DESC LIMIT 1000") {
            let guard = fake_state.run.lock().expect("run state");
            return Ok(match guard.as_ref() {
                Some(run) => stored_run_row(run),
                None => paged_rows(&[], None, None),
            });
        }
        if sql.starts_with("INSERT INTO runs ") {
            let values = quoted_values(sql);
            *fake_state.run.lock().expect("run state") = Some(StoredRun {
                id: values[0].clone(),
                session_id: values[1].clone(),
                status: values[7].clone(),
                input: values[8].clone(),
                output: values[9].clone(),
                error: values[10].clone(),
                metrics: values[11].clone(),
                stop_reason: values[12].clone(),
                iterations: 0,
            });
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("INSERT INTO traces ")
            || sql.starts_with("UPDATE traces SET ")
            || sql.starts_with("INSERT INTO spans ")
        {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("FROM spans") && sql.contains("status = 'failed'") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("INSERT INTO run_events ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("INSERT INTO usage ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("UPDATE runs SET status = ") {
            let values = quoted_values(sql);
            let run_id = values.last().cloned().unwrap_or_default();
            let mut guard = fake_state.run.lock().expect("run state");
            if let Some(run) = guard.as_mut() {
                if run.id == run_id {
                    run.status = values[0].clone();
                    if values.len() >= 6 {
                        run.output = values[1].clone();
                        run.error = values[2].clone();
                        run.metrics = values[3].clone();
                        run.stop_reason = values[4].clone();
                        run.iterations = sql
                            .split("iterations = ")
                            .nth(1)
                            .and_then(|rest| rest.split(',').next())
                            .and_then(|n| n.trim().parse::<u32>().ok())
                            .unwrap_or(run.iterations);
                    }
                }
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE id = ") {
            let guard = fake_state.run.lock().expect("run state");
            return Ok(match guard.as_ref() {
                Some(run) => stored_run_row(run),
                None => paged_rows(&[], None, None),
            });
        }
        if sql.starts_with("SELECT id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at) FROM run_events WHERE run_id = ") {
            let run_id = quoted_values(sql).first().cloned().unwrap_or_default();
            return Ok(run_event_rows(&run_id));
        }
        panic!("unexpected SQL in execute runs fast test: {sql}");
    });
    let prefix = format!(
        "test_fast_run_exec_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok from run")),
    )
    .await
}

#[tokio::test]
async fn runs_api_fast_list_get_and_events() -> Result<()> {
    let app = fake_runs_app().await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/agents/{agent_id}/runs?session_id=session-1&include_events=true"
                ))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"][0]["id"], "run-1");
    assert_eq!(list_body["data"][0]["status"], "COMPLETED");
    assert_eq!(
        list_body["data"][0]["events"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(list_body["data"][0]["events"][0]["event"], "Start");
    assert_eq!(list_body["data"][0]["events"][1]["event"], "ToolEnd");

    let detail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/runs/run-1"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = json_body(detail).await?;
    assert_eq!(detail_body["id"], "run-1");
    assert_eq!(detail_body["metrics"]["duration_ms"], 42);
    assert_eq!(detail_body["events"].as_array().map(Vec::len), Some(2));

    let events = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/runs/run-1/events"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(events.status(), StatusCode::OK);
    let events_body = json_body(events).await?;
    assert_eq!(events_body.as_array().map(Vec::len), Some(2));
    assert_eq!(events_body[0]["event"], "Start");
    assert_eq!(events_body[1]["event"], "ToolEnd");
    Ok(())
}

#[tokio::test]
async fn continue_run_rejects_non_paused_run_fast() -> Result<()> {
    let app = fake_runs_app().await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs/run-paused/continue"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "stream": false
                }))?))?,
        )
        .await?;

    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = json_body(resp).await?;
    assert!(body["error"]
        .as_str()
        .is_some_and(|msg| msg.contains("run is not paused")));
    Ok(())
}

#[tokio::test]
async fn create_run_non_stream_fast_executes_and_returns_completed_run() -> Result<()> {
    let state = RunExecState::default();
    let app = fake_execute_runs_app(state.clone()).await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "session_id": "session-1",
                    "input": "hello",
                    "stream": false
                }))?))?,
        )
        .await?;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["session_id"], "session-1");
    assert_eq!(body["input"], "hello");
    assert_eq!(body["output"], "ok from run");
    assert_eq!(body["status"], "COMPLETED");
    assert_eq!(body["events"].as_array().map(Vec::len), Some(2));

    let run = state.run.lock().expect("run state").clone();
    let run = run.expect("run should be stored");
    assert_eq!(run.status, "COMPLETED");
    assert_eq!(run.output, "ok from run");
    Ok(())
}

#[tokio::test]
async fn cancel_run_pending_fast_updates_status() -> Result<()> {
    let state = RunExecState {
        run: Arc::new(Mutex::new(Some(StoredRun {
            id: "run-pending".to_string(),
            session_id: "session-1".to_string(),
            status: "PENDING".to_string(),
            input: "hello".to_string(),
            output: String::new(),
            error: String::new(),
            metrics: String::new(),
            stop_reason: String::new(),
            iterations: 0,
        }))),
    };
    let app = fake_execute_runs_app(state.clone()).await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/runs/run-pending/cancel"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;

    assert_eq!(resp.status(), StatusCode::OK);
    let run = state.run.lock().expect("run state").clone().expect("run");
    assert_eq!(run.status, "CANCELLED");
    Ok(())
}
