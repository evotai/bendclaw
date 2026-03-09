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
async fn create_and_list_learnings() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("lrn-cl");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({
        "title": "Rust ownership",
        "content": "Ownership is Rust's most unique feature.",
        "tags": ["rust", "ownership"]
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/learnings"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["title"], "Rust ownership");
    let learning_id = created["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/learnings"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let list = json_body(resp2).await?;
    let items = list["data"].as_array().unwrap();
    assert!(items.iter().any(|l| l["id"] == learning_id.as_str()));
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn delete_learning() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("lrn-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({ "title": "To delete", "content": "content" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/learnings"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    let created = json_body(resp).await?;
    let learning_id = created["id"].as_str().unwrap().to_string();

    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/learnings/{learning_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body = json_body(resp2).await?;
    assert_eq!(body["deleted"], learning_id.as_str());
    ctx.teardown().await;
    Ok(())
}

#[tokio::test]
async fn search_learnings() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx.app_with_llm(Arc::new(MockLLMProvider::with_text("ok"))).await?;
    let agent_id = uid("lrn-srch");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({
        "title": "Async Rust",
        "content": "Tokio is an async runtime for Rust."
    });
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/learnings"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;

    let search = serde_json::json!({ "query": "async runtime" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/learnings/search"))
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

// ── serde unit tests ──

#[test]
fn learning_response_serializes_fields() {
    use bendclaw::service::v1::learnings::LearningResponse;

    let r = LearningResponse {
        id: "id1".into(),
        agent_id: "agent1".into(),
        user_id: "user1".into(),
        session_id: "sess1".into(),
        title: "Rust ownership".into(),
        content: "Ownership is unique.".into(),
        tags: vec!["rust".into(), "ownership".into()],
        source: "manual".into(),
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-02T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["id"], "id1");
    assert_eq!(v["agent_id"], "agent1");
    assert_eq!(v["title"], "Rust ownership");
    assert_eq!(v["source"], "manual");
    assert_eq!(v["tags"], serde_json::json!(["rust", "ownership"]));
}

#[test]
fn learning_response_empty_tags() {
    use bendclaw::service::v1::learnings::LearningResponse;

    let r = LearningResponse {
        id: "id2".into(),
        agent_id: "a".into(),
        user_id: "u".into(),
        session_id: "s".into(),
        title: "t".into(),
        content: "c".into(),
        tags: vec![],
        source: "manual".into(),
        created_at: "".into(),
        updated_at: "".into(),
    };
    let v = serde_json::to_value(&r).unwrap();
    assert_eq!(v["tags"], serde_json::json!([]));
}

#[test]
fn create_learning_request_default_source() {
    use bendclaw::service::v1::learnings::CreateLearningRequest;

    let req: CreateLearningRequest =
        serde_json::from_str(r#"{"title":"t","content":"c"}"#).unwrap();
    assert_eq!(req.source, "manual");
    assert!(req.tags.is_empty());
    assert!(req.session_id.is_empty());
}

#[test]
fn create_learning_request_with_tags() {
    use bendclaw::service::v1::learnings::CreateLearningRequest;

    let req: CreateLearningRequest =
        serde_json::from_str(r#"{"title":"t","content":"c","tags":["a","b"],"source":"auto"}"#)
            .unwrap();
    assert_eq!(req.tags, vec!["a", "b"]);
    assert_eq!(req.source, "auto");
}
