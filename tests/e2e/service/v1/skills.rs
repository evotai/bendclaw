use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::setup::json_body;
use bendclaw_test_harness::setup::setup_agent;
use bendclaw_test_harness::setup::uid;
use bendclaw_test_harness::setup::TestContext;
use tower::ServiceExt;

#[tokio::test]
async fn list_skills_empty() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-empty");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body.is_array());
    Ok(())
}

#[tokio::test]
async fn create_and_get_skill() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-cg");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let skill_name = uid("my-skill");
    let payload = serde_json::json!({
        "name": skill_name,
        "description": "A test skill",
        "content": "echo hello"
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["name"], skill_name.as_str());

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let got = json_body(resp2).await?;
    assert_eq!(got["name"], skill_name.as_str());
    assert_eq!(got["content"], "echo hello");
    Ok(())
}

#[tokio::test]
async fn delete_skill() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let skill_name = uid("del-skill");
    let payload = serde_json::json!({
        "name": skill_name,
        "description": "to delete",
        "content": "echo bye"
    });
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/skills"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["deleted"], skill_name.as_str());
    Ok(())
}

#[tokio::test]
async fn get_skill_not_found() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("sk-nf");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/skills/nonexistent-skill"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

// ── SkillResponse serde ──

#[test]
fn skill_response_serializes_fields() -> anyhow::Result<()> {
    let s = bendclaw::service::v1::skills::SkillResponse {
        name: "my-skill".into(),
        version: "1.0.0".into(),
        scope: "agent".into(),
        source: "agent".into(),
        description: "does things".into(),
        executable: true,
    };
    let v = serde_json::to_value(&s)?;
    assert_eq!(v["name"], "my-skill");
    assert_eq!(v["version"], "1.0.0");
    assert_eq!(v["scope"], "agent");
    assert_eq!(v["source"], "agent");
    assert_eq!(v["description"], "does things");
    assert_eq!(v["executable"], true);
    Ok(())
}

#[test]
fn skill_response_non_executable() -> anyhow::Result<()> {
    let s = bendclaw::service::v1::skills::SkillResponse {
        name: "read-only".into(),
        version: "0.0.1".into(),
        scope: "global".into(),
        source: "builtin".into(),
        description: String::new(),
        executable: false,
    };
    let v = serde_json::to_value(&s)?;
    assert_eq!(v["executable"], false);
    assert_eq!(v["version"], "0.0.1");
    Ok(())
}

#[test]
fn skill_detail_response_serializes_content() -> anyhow::Result<()> {
    let s = bendclaw::service::v1::skills::SkillDetailResponse {
        name: "shell".into(),
        version: "1.0.0".into(),
        scope: "global".into(),
        source: "builtin".into(),
        description: "run shell commands".into(),
        content: "#!/bin/bash\necho hello".into(),
        executable: true,
    };
    let v = serde_json::to_value(&s)?;
    assert_eq!(v["name"], "shell");
    assert_eq!(v["content"], "#!/bin/bash\necho hello");
    assert_eq!(v["executable"], true);
    Ok(())
}
