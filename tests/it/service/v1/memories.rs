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
async fn create_and_list_memories() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("mem-cl");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "key": "test-key", "content": "test content" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/memories"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["key"], "test-key");
    let memory_id = created["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/memories"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let list = json_body(resp2).await?;
    let items = list["data"].as_array().unwrap();
    assert!(items.iter().any(|m| m["id"] == memory_id.as_str()));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn get_memory_by_id() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("mem-get");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "key": "my-key", "content": "my content" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/memories"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let memory_id = created["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/memories/{memory_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let got = json_body(resp2).await?;
    assert_eq!(got["key"], "my-key");
    assert_eq!(got["content"], "my content");
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn delete_memory() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("mem-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "key": "del-key", "content": "to delete" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/memories"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let memory_id = created["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/memories/{memory_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = json_body(resp2).await?;
    assert_eq!(body["deleted"], memory_id.as_str());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn search_memories() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("mem-srch");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "key": "rust-tip", "content": "Rust is fast and safe" });
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/memories"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;

    let search = serde_json::json!({ "query": "Rust programming" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/memories/search"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&search)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert!(body.is_array());
    ctx.teardown().await;
    Ok(())
}

// ── MemoryResponse serde ──

#[test]
fn memory_response_serializes_fields() {
    use bendclaw::kernel::agent_store::memory_store::MemoryScope;
    let m = bendclaw::service::v1::memories::MemoryResponse {
        id: "mem-1".into(),
        scope: MemoryScope::User,
        session_id: None,
        key: "my-key".into(),
        content: "my content".into(),
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-01T00:01:00Z".into(),
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["id"], "mem-1");
    assert_eq!(v["scope"], "user");
    assert!(v["session_id"].is_null());
    assert_eq!(v["key"], "my-key");
    assert_eq!(v["content"], "my content");
    assert_eq!(v["created_at"], "2024-01-01T00:00:00Z");
    assert_eq!(v["updated_at"], "2024-01-01T00:01:00Z");
}

#[test]
fn memory_response_session_scope() {
    use bendclaw::kernel::agent_store::memory_store::MemoryScope;
    let m = bendclaw::service::v1::memories::MemoryResponse {
        id: "mem-2".into(),
        scope: MemoryScope::Session,
        session_id: Some("sess-abc".into()),
        key: "ctx".into(),
        content: "session data".into(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["scope"], "session");
    assert_eq!(v["session_id"], "sess-abc");
}

#[test]
fn memory_response_shared_scope() {
    use bendclaw::kernel::agent_store::memory_store::MemoryScope;
    let m = bendclaw::service::v1::memories::MemoryResponse {
        id: "mem-3".into(),
        scope: MemoryScope::Shared,
        session_id: None,
        key: "shared-key".into(),
        content: "shared content".into(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    let v = serde_json::to_value(&m).unwrap();
    assert_eq!(v["scope"], "shared");
}
