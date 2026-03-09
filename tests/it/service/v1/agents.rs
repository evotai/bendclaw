use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::TestContext;
use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn list_agents_empty() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let user = uid("user");
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/agents")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["data"].is_array());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn get_agent_not_found() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let user = uid("user");
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/agents/nonexistent-agent-xyz")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn setup_and_get_agent() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("ag-get");
    let user = uid("user");

    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["agent_id"], agent_id.as_str());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn list_agents_includes_setup_agent() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("ag-list");
    let user = uid("user");

    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/agents")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    let agents = body["data"].as_array().unwrap();
    assert!(agents.iter().any(|a| a["agent_id"] == agent_id.as_str()));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn delete_agent() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("ag-del");
    let user = uid("user");

    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["deleted"], agent_id.as_str());
    ctx.teardown().await;
    Ok(())
}

// ── AgentEntry / AgentDetail serde ──

#[test]
fn agent_entry_serializes_fields() {
    let e = bendclaw::service::v1::agents::AgentEntry {
        agent_id: "agent-1".into(),
        display_name: "My Agent".into(),
        description: "does stuff".into(),
    };
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["agent_id"], "agent-1");
    assert_eq!(v["display_name"], "My Agent");
    assert_eq!(v["description"], "does stuff");
}

#[test]
fn agent_detail_serializes_fields() {
    let d = bendclaw::service::v1::agents::AgentDetail {
        agent_id: "agent-2".into(),
        display_name: "Detail Agent".into(),
        description: "detailed".into(),
        system_prompt: "You are helpful.".into(),
        identity: "assistant".into(),
        soul: String::new(),
        token_limit_total: Some(100_000),
        token_limit_daily: None,
    };
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["agent_id"], "agent-2");
    assert_eq!(v["system_prompt"], "You are helpful.");
    assert_eq!(v["identity"], "assistant");
    assert_eq!(v["token_limit_total"], 100_000);
}

#[test]
fn agent_detail_empty_fields() {
    let d = bendclaw::service::v1::agents::AgentDetail {
        agent_id: "agent-3".into(),
        display_name: String::new(),
        description: String::new(),
        system_prompt: String::new(),
        identity: String::new(),
        soul: String::new(),
        token_limit_total: None,
        token_limit_daily: None,
    };
    let v = serde_json::to_value(&d).unwrap();
    assert!(v["token_limit_total"].is_null());
}
