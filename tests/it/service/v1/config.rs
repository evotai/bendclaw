use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::setup::json_body;
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn get_config_default() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("cfg-get");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["agent_id"], agent_id.as_str());
    Ok(())
}

#[tokio::test]
async fn update_and_get_config() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("cfg-upd");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({
        "system_prompt": "You are a helpful assistant.",
        "display_name": "Test Agent",
        "description": "A test agent"
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["version"], 1);

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let cfg = json_body(resp2).await?;
    assert_eq!(cfg["system_prompt"], "You are a helpful assistant.");
    assert_eq!(cfg["display_name"], "Test Agent");
    Ok(())
}

#[tokio::test]
async fn list_config_versions() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("cfg-ver");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    for i in 1..=2u32 {
        let payload = serde_json::json!({ "system_prompt": format!("prompt v{i}") });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/v1/agents/{agent_id}/config"))
                    .header("content-type", "application/json")
                    .header("x-user-id", &user)
                    .body(Body::from(serde_json::to_vec(&payload)?))?,
            )
            .await?;
    }

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/config/versions"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["data"].as_array().context("expected data array")?.len() >= 2);
    Ok(())
}

#[tokio::test]
async fn get_specific_version() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("cfg-sv");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "system_prompt": "v1 prompt", "label": "initial" });
    app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/config/versions/1"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["version"], 1);
    assert_eq!(body["system_prompt"], "v1 prompt");
    Ok(())
}

#[tokio::test]
async fn rollback_config() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("cfg-rb");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let p1 = serde_json::json!({ "system_prompt": "original prompt" });
    app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&p1)?))?,
        )
        .await?;

    let p2 = serde_json::json!({ "system_prompt": "updated prompt" });
    app.clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&p2)?))?,
        )
        .await?;

    let rb = serde_json::json!({ "version": 1 });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/config/rollback"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&rb)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body["ok"], true);
    assert_eq!(body["rolled_back_to"], 1);

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/config"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let cfg = json_body(resp2).await?;
    assert_eq!(cfg["system_prompt"], "original prompt");
    Ok(())
}

// ── serde unit tests ──

#[test]
fn config_response_serializes_fields() -> anyhow::Result<()> {
    use std::collections::HashMap;

    use bendclaw::service::v1::config::ConfigResponse;

    let mut env = HashMap::new();
    env.insert("KEY".to_string(), "val".to_string());
    let r = ConfigResponse {
        agent_id: "agent1".into(),
        system_prompt: "You are helpful.".into(),
        display_name: "Test".into(),
        description: "desc".into(),
        identity: String::new(),
        soul: String::new(),
        token_limit_total: None,
        token_limit_daily: None,
        env,
    };
    let v = serde_json::to_value(&r)?;
    assert_eq!(v["agent_id"], "agent1");
    assert_eq!(v["system_prompt"], "You are helpful.");
    assert_eq!(v["display_name"], "Test");
    assert_eq!(v["env"]["KEY"], "val");
    Ok(())
}

#[test]
fn config_response_null_meta_empty_env() -> anyhow::Result<()> {
    use std::collections::HashMap;

    use bendclaw::service::v1::config::ConfigResponse;

    let r = ConfigResponse {
        agent_id: "a".into(),
        system_prompt: "".into(),
        display_name: "".into(),
        description: "".into(),
        identity: String::new(),
        soul: String::new(),
        token_limit_total: None,
        token_limit_daily: None,
        env: HashMap::new(),
    };
    let v = serde_json::to_value(&r)?;
    assert!(v["token_limit_total"].is_null());
    assert_eq!(v["env"], serde_json::json!({}));
    Ok(())
}

#[test]
fn version_response_serializes_fields() -> anyhow::Result<()> {
    use bendclaw::service::v1::config::VersionResponse;

    let r = VersionResponse {
        id: "vid1".into(),
        version: 3,
        label: "stable".into(),
        stage: "published".into(),
        system_prompt: "prompt".into(),
        display_name: "Agent".into(),
        description: "desc".into(),
        identity: String::new(),
        soul: String::new(),
        token_limit_total: None,
        token_limit_daily: None,
        notes: "initial release".into(),
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&r)?;
    assert_eq!(v["id"], "vid1");
    assert_eq!(v["version"], 3);
    assert_eq!(v["label"], "stable");
    assert_eq!(v["stage"], "published");
    assert_eq!(v["notes"], "initial release");
    Ok(())
}

#[test]
fn rollback_request_deserializes() -> anyhow::Result<()> {
    use bendclaw::service::v1::config::RollbackRequest;

    let req: RollbackRequest = serde_json::from_str(r#"{"version": 2}"#)?;
    assert_eq!(req.version, 2);
    Ok(())
}

#[test]
fn update_config_request_all_optional() -> anyhow::Result<()> {
    use bendclaw::service::v1::config::UpdateConfigRequest;

    let req: UpdateConfigRequest = serde_json::from_str(r#"{}"#)?;
    assert!(req.system_prompt.is_none());
    assert!(req.display_name.is_none());
    assert!(req.description.is_none());
    assert!(req.identity.is_none());
    assert!(req.env.is_none());
    assert!(req.notes.is_none());
    assert!(req.label.is_none());
    Ok(())
}

#[test]
fn update_config_request_partial() -> anyhow::Result<()> {
    use bendclaw::service::v1::config::UpdateConfigRequest;

    let req: UpdateConfigRequest =
        serde_json::from_str(r#"{"system_prompt":"hello","label":"v2"}"#)?;
    assert_eq!(req.system_prompt.as_deref(), Some("hello"));
    assert_eq!(req.label.as_deref(), Some("v2"));
    assert!(req.display_name.is_none());
    Ok(())
}
