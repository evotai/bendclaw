use axum::http::HeaderMap;

use super::account_service::record_to_domain;
use crate::kernel::channels::runtime::channel_trait::InboundKind;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;

pub struct ChannelIngressService {
    state: AppState,
}

impl ChannelIngressService {
    pub fn new(state: &AppState) -> Self {
        Self {
            state: state.clone(),
        }
    }

    /// Handle an inbound webhook. Returns Some(json) for challenge responses, None otherwise.
    pub async fn handle_webhook(
        &self,
        agent_id: &str,
        channel_account_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<Option<serde_json::Value>> {
        let pool = self.state.runtime.databases().agent_pool(agent_id)?;
        let repo = ChannelAccountRepo::new(pool.clone());

        let record = repo.load(channel_account_id).await?.ok_or_else(|| {
            ServiceError::AgentNotFound(format!("channel account '{channel_account_id}' not found"))
        })?;

        if !record.enabled {
            return Err(ServiceError::BadRequest(
                "channel account is disabled".into(),
            ));
        }

        let registry = self.state.runtime.channels();
        let entry = registry.get(&record.channel_type).ok_or_else(|| {
            ServiceError::BadRequest(format!("unknown channel type: {}", record.channel_type))
        })?;

        let wh = match &entry.inbound {
            InboundKind::Webhook(wh) => wh.clone(),
            _ => {
                return Err(ServiceError::BadRequest(format!(
                    "channel '{}' does not support webhooks",
                    record.channel_type
                )))
            }
        };

        // Challenge handshake (e.g. GitHub webhook ping).
        if let Some(challenge) = wh.challenge_response(body) {
            return Ok(Some(challenge));
        }

        wh.verify(&record.account_id, headers, body)
            .map_err(|e| ServiceError::BadRequest(format!("webhook verification failed: {e}")))?;

        let events = wh
            .parse(&record.account_id, body)
            .map_err(|e| ServiceError::BadRequest(format!("webhook parse failed: {e}")))?;

        if events.is_empty() {
            return Ok(None);
        }

        let runtime = self.state.runtime.clone();
        let account = record_to_domain(&record);

        // Process events in background so the webhook returns 200 quickly.
        // Sequential route().await preserves event ordering within the batch.
        crate::types::spawn_fire_and_forget("webhook_event_dispatch", async move {
            for event in events {
                runtime.chat_router().route(account.clone(), event).await;
            }
        });

        Ok(None)
    }
}
