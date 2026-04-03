use axum::body::Bytes;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::account_service::ChannelAccountService;
use super::ingress_service::ChannelIngressService;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

// ── Request / Response types ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateChannelAccountRequest {
    pub channel_type: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_account_id: Option<String>,
    #[serde(default = "default_config")]
    pub config: serde_json::Value,
    pub enabled: Option<bool>,
}

fn default_config() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Serialize)]
pub struct ChannelAccountView {
    pub id: String,
    pub channel_type: String,
    pub external_account_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ChannelMessageResponse {
    pub id: String,
    pub channel_type: String,
    pub account_id: String,
    pub chat_id: String,
    pub session_id: String,
    pub direction: String,
    pub sender_id: String,
    pub text: String,
    pub platform_message_id: String,
    pub run_id: String,
    pub created_at: String,
}

#[derive(Deserialize, Default)]
pub struct MessagesQuery {
    pub channel_type: Option<String>,
    pub chat_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<u64>,
}

// ── Handlers ─────────────────────────────────────────────────────────────

pub async fn create_account(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateChannelAccountRequest>,
) -> Result<Json<ChannelAccountView>> {
    Ok(Json(
        ChannelAccountService::new(&state)
            .create(&agent_id, req)
            .await?,
    ))
}

pub async fn list_accounts(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<Vec<ChannelAccountView>>> {
    Ok(Json(
        ChannelAccountService::new(&state).list(&agent_id).await?,
    ))
}

pub async fn get_account(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, account_id)): Path<(String, String)>,
) -> Result<Json<ChannelAccountView>> {
    Ok(Json(
        ChannelAccountService::new(&state)
            .get(&agent_id, &account_id)
            .await?,
    ))
}

pub async fn delete_account(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, account_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    ChannelAccountService::new(&state)
        .delete(&agent_id, &account_id)
        .await?;
    Ok(Json(serde_json::json!({})))
}

pub async fn list_messages(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<Vec<ChannelMessageResponse>>> {
    let pool = state.runtime.databases().agent_pool(&agent_id)?;
    let repo = ChannelMessageRepo::new(pool);

    let records = if let Some(sid) = q.session_id.as_deref() {
        repo.list_by_session(sid, q.limit.unwrap_or(100)).await?
    } else if let (Some(ct), Some(cid)) = (q.channel_type.as_deref(), q.chat_id.as_deref()) {
        repo.list_by_chat(ct, cid, q.limit.unwrap_or(100)).await?
    } else {
        return Err(ServiceError::BadRequest(
            "either session_id or (channel_type + chat_id) is required".to_string(),
        ));
    };

    let views = records
        .into_iter()
        .map(|r| ChannelMessageResponse {
            id: r.id,
            channel_type: r.channel_type,
            account_id: r.account_id,
            chat_id: r.chat_id,
            session_id: r.session_id,
            direction: r.direction,
            sender_id: r.sender_id,
            text: r.text,
            platform_message_id: r.platform_message_id,
            run_id: r.run_id,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(views))
}

/// Webhook endpoint — no auth middleware, external platforms call this directly.
pub async fn webhook(
    State(state): State<AppState>,
    Path((agent_id, account_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>> {
    let result = ChannelIngressService::new(&state)
        .handle_webhook(&agent_id, &account_id, &headers, &body)
        .await?;
    match result {
        Some(challenge) => Ok(Json(challenge)),
        None => Ok(Json(serde_json::json!({ "ok": true }))),
    }
}
