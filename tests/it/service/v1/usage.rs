use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::TestContext;
use crate::common::setup::chat;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn usage_summary_zero_for_new_agent() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("usg-zero");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/usage"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["record_count"], 0);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn usage_summary_after_chat() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("usg-chat");
    let user = uid("user");
    let session_id = uid("session");
    setup_agent(&app, &agent_id, &user).await?;
    chat(&app, &agent_id, &session_id, &user, "hello").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/usage"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["record_count"].as_u64().unwrap_or(0) >= 1);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn usage_daily_returns_array() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("usg-daily");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/usage/daily"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body.is_array());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn global_usage_summary() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let user = uid("user");

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/usage/summary")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["total_tokens"].is_number());
    ctx.teardown().await;
    Ok(())
}

// ── serde unit tests ──

#[test]
fn usage_summary_response_serializes_fields() {
    use bendclaw::service::v1::usage::UsageSummaryResponse;

    let r = UsageSummaryResponse {
        total_prompt_tokens: 100,
        total_completion_tokens: 200,
        total_reasoning_tokens: 50,
        total_tokens: 350,
        total_cost: 0.042,
        record_count: 5,
        total_cache_read_tokens: 10,
        total_cache_write_tokens: 20,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["total_prompt_tokens"], 100);
    assert_eq!(v["total_completion_tokens"], 200);
    assert_eq!(v["total_reasoning_tokens"], 50);
    assert_eq!(v["total_tokens"], 350);
    assert_eq!(v["record_count"], 5);
    assert_eq!(v["total_cache_read_tokens"], 10);
    assert_eq!(v["total_cache_write_tokens"], 20);
}

#[test]
fn usage_summary_response_zero_values() {
    use bendclaw::service::v1::usage::UsageSummaryResponse;

    let r = UsageSummaryResponse {
        total_prompt_tokens: 0,
        total_completion_tokens: 0,
        total_reasoning_tokens: 0,
        total_tokens: 0,
        total_cost: 0.0,
        record_count: 0,
        total_cache_read_tokens: 0,
        total_cache_write_tokens: 0,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["record_count"], 0);
    assert_eq!(v["total_cost"], 0.0);
}

#[test]
fn daily_usage_response_serializes_fields() {
    use bendclaw::service::v1::usage::DailyUsageResponse;

    let r = DailyUsageResponse {
        date: "2024-01-15".into(),
        prompt_tokens: 80,
        completion_tokens: 120,
        total_tokens: 200,
        cost: 0.01,
        requests: 3,
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["date"], "2024-01-15");
    assert_eq!(v["prompt_tokens"], 80);
    assert_eq!(v["completion_tokens"], 120);
    assert_eq!(v["total_tokens"], 200);
    assert_eq!(v["requests"], 3);
}

#[test]
fn daily_query_default_days_none() {
    use bendclaw::service::v1::usage::DailyQuery;

    let q: DailyQuery = serde_json::from_str(r#"{}"#).unwrap();
    assert!(q.days.is_none());
}

#[test]
fn daily_query_with_days() {
    use bendclaw::service::v1::usage::DailyQuery;

    let q: DailyQuery = serde_json::from_str(r#"{"days": 30}"#).unwrap();
    assert_eq!(q.days, Some(30));
}
