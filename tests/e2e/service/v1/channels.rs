use std::sync::Arc;

use anyhow::Context as _;
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

// ── Helpers ──

async fn create_channel_account(
    app: &axum::Router,
    agent_id: &str,
    user: &str,
    channel_type: &str,
    account_user_id: &str,
) -> Result<serde_json::Value> {
    let payload = serde_json::json!({
        "channel_type": channel_type,
        "user_id": account_user_id,
        "config": { "token": "test-token" },
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/channels/accounts"))
                .header("content-type", "application/json")
                .header("x-user-id", user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    json_body(resp).await
}

// ── CRUD tests ──

#[tokio::test]
async fn create_and_get_channel_account() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ch-cg");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let created = create_channel_account(&app, &agent_id, &user, "telegram", &user).await?;
    assert_eq!(created["channel_type"], "telegram");
    assert_eq!(created["enabled"], true);
    assert!(!created["id"].as_str().context("missing id")?.is_empty());
    let account_id = created["id"].as_str().context("missing id")?.to_string();

    // GET by id
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/agents/{agent_id}/channels/accounts/{account_id}"
                ))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let got = json_body(resp).await?;
    assert_eq!(got["id"], account_id.as_str());
    assert_eq!(got["channel_type"], "telegram");
    assert_eq!(got["config"]["token"], "***");
    Ok(())
}

#[tokio::test]
async fn list_channel_accounts() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ch-list");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    create_channel_account(&app, &agent_id, &user, "telegram", &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/channels/accounts"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    let status = resp.status();
    let body = json_body(resp).await?;
    assert_eq!(status, StatusCode::OK, "list accounts failed: {body}");
    let accounts = body.as_array().context("expected JSON array")?;
    assert!(!accounts.is_empty(), "expected at least one account");
    assert_eq!(accounts[0]["channel_type"], "telegram");
    Ok(())
}

#[tokio::test]
async fn delete_channel_account() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ch-del");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let created = create_channel_account(&app, &agent_id, &user, "telegram", &user).await?;
    let account_id = created["id"].as_str().context("missing id")?.to_string();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/v1/agents/{agent_id}/channels/accounts/{account_id}"
                ))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await?;
    assert_eq!(body, serde_json::json!({}));
    Ok(())
}

#[tokio::test]
async fn create_account_with_custom_account_id() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ch-custom");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let payload = serde_json::json!({
        "channel_type": "github",
        "user_id": user,
        "external_account_id": "my-github-bot",
        "config": { "token": "ghp_test", "webhook_secret": "s3cret" },
        "enabled": false,
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/channels/accounts"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&payload)?))?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::OK);
    let created = json_body(resp).await?;
    assert_eq!(created["external_account_id"], "my-github-bot");
    assert_eq!(created["enabled"], false);
    Ok(())
}

#[tokio::test]
async fn list_messages_requires_filter() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let app = ctx
        .app_with_llm(Arc::new(MockLLMProvider::with_text("ok")))
        .await?;
    let agent_id = uid("ch-msg-err");
    let user = uid("user");
    setup_agent(&app, &agent_id, &user).await?;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/channels/messages"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

// ── Request / Response serde tests ──

#[test]
fn create_request_all_fields() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::channels::http::CreateChannelAccountRequest =
        serde_json::from_str(
            r#"{
                "channel_type": "telegram",
                "user_id": "u1",
                "external_account_id": "custom-id",
                "config": {"bot_token": "abc"},
                "enabled": false
            }"#,
        )?;
    assert_eq!(r.channel_type, "telegram");
    assert_eq!(r.user_id, "u1");
    assert_eq!(r.external_account_id.as_deref(), Some("custom-id"));
    assert_eq!(r.config["bot_token"], "abc");
    assert_eq!(r.enabled, Some(false));
    Ok(())
}

#[test]
fn create_request_minimal() -> anyhow::Result<()> {
    let r: bendclaw::service::v1::channels::http::CreateChannelAccountRequest =
        serde_json::from_str(r#"{"channel_type": "feishu", "user_id": "u2"}"#)?;
    assert_eq!(r.channel_type, "feishu");
    assert_eq!(r.user_id, "u2");
    assert!(r.external_account_id.is_none());
    assert!(r.config.is_object());
    assert!(r.enabled.is_none());
    Ok(())
}

#[test]
fn account_view_serializes_all_fields() -> anyhow::Result<()> {
    let r = bendclaw::service::v1::channels::http::ChannelAccountView {
        id: "ca_1".into(),
        channel_type: "telegram".into(),
        external_account_id: "acc_1".into(),
        agent_id: "agent_1".into(),
        user_id: "user_1".into(),
        config: serde_json::json!({"token": "t"}),
        enabled: true,
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-02T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&r)?;
    assert_eq!(v["id"], "ca_1");
    assert_eq!(v["channel_type"], "telegram");
    assert_eq!(v["external_account_id"], "acc_1");
    assert_eq!(v["agent_id"], "agent_1");
    assert_eq!(v["config"]["token"], "t");
    assert_eq!(v["enabled"], true);
    Ok(())
}

#[test]
fn message_response_serializes_all_fields() -> anyhow::Result<()> {
    let r = bendclaw::service::v1::channels::http::ChannelMessageResponse {
        id: "cm_1".into(),
        channel_type: "telegram".into(),
        account_id: "acc_1".into(),
        chat_id: "chat_1".into(),
        session_id: "sess_1".into(),
        direction: "inbound".into(),
        sender_id: "user_1".into(),
        text: "hello".into(),
        platform_message_id: "tg_123".into(),
        run_id: "run_1".into(),
        created_at: "2024-01-01T00:00:00Z".into(),
    };
    let v = serde_json::to_value(&r)?;
    assert_eq!(v["id"], "cm_1");
    assert_eq!(v["direction"], "inbound");
    assert_eq!(v["text"], "hello");
    assert_eq!(v["platform_message_id"], "tg_123");
    Ok(())
}

#[test]
fn messages_query_defaults() {
    let q = bendclaw::service::v1::channels::http::MessagesQuery::default();
    assert!(q.channel_type.is_none());
    assert!(q.chat_id.is_none());
    assert!(q.session_id.is_none());
    assert!(q.limit.is_none());
}

#[test]
fn messages_query_with_session_id() -> anyhow::Result<()> {
    let q: bendclaw::service::v1::channels::http::MessagesQuery =
        serde_json::from_str(r#"{"session_id": "sess_1", "limit": 50}"#)?;
    assert_eq!(q.session_id.as_deref(), Some("sess_1"));
    assert_eq!(q.limit, Some(50));
    Ok(())
}

#[test]
fn messages_query_with_chat_filter() -> anyhow::Result<()> {
    let q: bendclaw::service::v1::channels::http::MessagesQuery =
        serde_json::from_str(r#"{"channel_type": "telegram", "chat_id": "chat_42"}"#)?;
    assert_eq!(q.channel_type.as_deref(), Some("telegram"));
    assert_eq!(q.chat_id.as_deref(), Some("chat_42"));
    Ok(())
}
