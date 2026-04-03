use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn traces_api_fast_list_get_and_spans() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("SELECT COUNT(*) FROM traces WHERE ") {
            return Ok(paged_rows(&[&["1"]], None, None));
        }
        if sql.starts_with("SELECT trace_id, run_id, session_id")
            && sql.contains("ORDER BY created_at DESC")
        {
            return Ok(paged_rows(
                &[&[
                    "trace-1",
                    "run-1",
                    "session-1",
                    "agent-a",
                    "user-a",
                    "agent.run",
                    "completed",
                    "42",
                    "10",
                    "20",
                    "0.5",
                    "",
                    "",
                    "2026-03-11T00:00:00Z",
                    "2026-03-11T00:01:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.starts_with("SELECT trace_id, run_id, session_id")
            && sql.contains("WHERE trace_id = 'trace-1' LIMIT 1")
        {
            return Ok(paged_rows(
                &[&[
                    "trace-1",
                    "run-1",
                    "session-1",
                    "agent-a",
                    "user-a",
                    "agent.run",
                    "completed",
                    "42",
                    "10",
                    "20",
                    "0.5",
                    "",
                    "",
                    "2026-03-11T00:00:00Z",
                    "2026-03-11T00:01:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.starts_with("SELECT span_id, trace_id, parent_span_id") {
            return Ok(paged_rows(
                &[&[
                    "span-1",
                    "trace-1",
                    "",
                    "shell",
                    "tool",
                    "assistant",
                    "completed",
                    "12",
                    "3",
                    "4",
                    "5",
                    "0",
                    "0.25",
                    "",
                    "",
                    "echo hi",
                    "{}",
                    "2026-03-11T00:00:30Z",
                ]],
                None,
                None,
            ));
        }
        Ok(paged_rows(&[], None, None))
    });

    let prefix = format!(
        "test_fast_trace_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"][0]["trace_id"], "trace-1");
    assert_eq!(list_body["data"][0]["status"], "completed");
    // parent_trace_id / origin_node_id are empty → skip_serializing_if omits them
    assert!(list_body["data"][0].get("parent_trace_id").is_none());
    assert!(list_body["data"][0].get("origin_node_id").is_none());

    let detail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces/trace-1"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_body = json_body(detail).await?;
    assert_eq!(detail_body["trace"]["trace_id"], "trace-1");
    assert_eq!(detail_body["spans"][0]["span_id"], "span-1");
    assert_eq!(detail_body["spans"][0]["kind"], "tool");

    let spans = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces/trace-1/spans"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(spans.status(), StatusCode::OK);
    let spans_body = json_body(spans).await?;
    assert_eq!(spans_body[0]["span_id"], "span-1");
    assert_eq!(spans_body[0]["summary"], "echo hi");
    Ok(())
}

#[tokio::test]
async fn traces_api_list_child_traces() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.contains("WHERE parent_trace_id =") {
            return Ok(paged_rows(
                &[&[
                    "child-t",
                    "run-2",
                    "session-2",
                    "agent-b",
                    "user-a",
                    "agent.run",
                    "completed",
                    "30",
                    "5",
                    "8",
                    "0.1",
                    "parent-t",
                    "node-1",
                    "2026-03-11T00:02:00Z",
                    "2026-03-11T00:03:00Z",
                ]],
                None,
                None,
            ));
        }
        Ok(paged_rows(&[], None, None))
    });

    let prefix = format!(
        "test_child_trace_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/traces/parent-t/children"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body[0]["trace_id"], "child-t");
    assert_eq!(body[0]["parent_trace_id"], "parent-t");
    assert_eq!(body[0]["origin_node_id"], "node-1");
    Ok(())
}
