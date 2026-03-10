use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use bendclaw_test_harness::setup::chat;
use bendclaw_test_harness::setup::json_body;
use bendclaw_test_harness::setup::setup_agent;
use bendclaw_test_harness::setup::uid;
use bendclaw_test_harness::setup::TestContext;
use tower::ServiceExt;

#[tokio::test]
async fn list_sessions_empty() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ses-empty");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body["data"]
        .as_array()
        .context("expected data array")?
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn create_and_get_session() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ses-cg");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "title": "My Session" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/sessions"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    let session_id = created["id"].as_str().context("missing id")?.to_string();
    assert_eq!(created["title"], "My Session");

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let got = json_body(resp2).await?;
    assert_eq!(got["id"], session_id.as_str());
    Ok(())
}

#[tokio::test]
async fn update_session_title() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ses-upd");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "title": "Original" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/sessions"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let session_id = created["id"].as_str().context("missing id")?.to_string();

    let update = serde_json::json!({ "title": "Updated Title" });
    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&update)?))?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let updated = json_body(resp2).await?;
    assert_eq!(updated["title"], "Updated Title");
    Ok(())
}

#[tokio::test]
async fn delete_session() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ses-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "title": "To Delete" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/sessions"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let session_id = created["id"].as_str().context("missing id")?.to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/sessions/{session_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = json_body(resp2).await?;
    assert_eq!(body["deleted"], session_id.as_str());
    Ok(())
}

#[tokio::test]
async fn list_sessions_search() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ses-srch");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let session_id = uid("session");
    chat(&app, &agent_id, &session_id, &user, "find me please").await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/sessions?search=find+me"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    let sessions = body["data"].as_array().context("expected data array")?;
    assert!(sessions.iter().any(|s| s["id"] == session_id.as_str()));
    Ok(())
}

// ── SessionResponse serde ──

#[test]
fn session_response_serializes_all_fields() -> anyhow::Result<()> {
    let s = bendclaw::service::v1::sessions::SessionResponse {
        id: "sid-1".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        title: "Hello".into(),
        session_state: serde_json::json!({"key": "val"}),
        meta: serde_json::json!({"x": 1}),
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-02T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&s)?;
    assert_eq!(v["id"], "sid-1");
    assert_eq!(v["agent_id"], "agent-1");
    assert_eq!(v["user_id"], "user-1");
    assert_eq!(v["title"], "Hello");
    assert_eq!(v["session_state"]["key"], "val");
    assert_eq!(v["meta"]["x"], 1);
    assert_eq!(v["created_at"], "2024-01-01T00:00:00Z");
    assert_eq!(v["updated_at"], "2024-01-02T00:00:00Z");
    Ok(())
}

#[test]
fn session_response_null_json_fields() -> anyhow::Result<()> {
    let s = bendclaw::service::v1::sessions::SessionResponse {
        id: "sid-2".into(),
        agent_id: "agent-2".into(),
        user_id: "user-2".into(),
        title: String::new(),
        session_state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let v = serde_json::to_value(&s)?;
    assert!(v["session_state"].is_null());
    assert!(v["meta"].is_null());
    Ok(())
}

// ── SessionsQuery deserialization ──

#[test]
fn sessions_query_defaults() {
    let q = bendclaw::service::v1::sessions::SessionsQuery::default();
    assert!(q.search.is_none());
}

#[test]
fn sessions_query_with_search() -> anyhow::Result<()> {
    let q: bendclaw::service::v1::sessions::SessionsQuery =
        serde_json::from_str(r#"{"search": "hello"}"#)?;
    assert_eq!(q.search.as_deref(), Some("hello"));
    Ok(())
}

// ── CreateSessionRequest deserialization ──

#[test]
fn create_session_request_all_fields() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::sessions::CreateSessionRequest =
        serde_json::from_str(r#"{"title": "My Session", "session_state": {"k": 1}}"#)?;
    assert_eq!(r.title.as_deref(), Some("My Session"));
    assert_eq!(
        r.session_state.as_ref().context("missing session_state")?["k"],
        1
    );
    Ok(())
}

#[test]
fn create_session_request_empty() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::sessions::CreateSessionRequest = serde_json::from_str(r#"{}"#)?;
    assert!(r.title.is_none());
    assert!(r.session_state.is_none());
    Ok(())
}

// ── UpdateSessionRequest deserialization ──

#[test]
fn update_session_request_title_only() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::sessions::UpdateSessionRequest =
        serde_json::from_str(r#"{"title": "New Title"}"#)?;
    assert_eq!(r.title.as_deref(), Some("New Title"));
    assert!(r.session_state.is_none());
    Ok(())
}

#[test]
fn update_session_request_state_only() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::sessions::UpdateSessionRequest =
        serde_json::from_str(r#"{"session_state": {"mode": "active"}}"#)?;
    assert!(r.title.is_none());
    assert_eq!(
        r.session_state.as_ref().context("missing session_state")?["mode"],
        "active"
    );
    Ok(())
}

#[test]
fn update_session_request_empty() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::sessions::UpdateSessionRequest = serde_json::from_str(r#"{}"#)?;
    assert!(r.title.is_none());
    assert!(r.session_state.is_none());
    Ok(())
}
