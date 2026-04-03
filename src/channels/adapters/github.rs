use std::sync::Arc;

use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::Deserialize;
use serde::Serialize;

use crate::channels::model::capabilities::ChannelCapabilities;
use crate::channels::model::capabilities::ChannelKind;
use crate::channels::model::capabilities::InboundMode;
use crate::channels::model::message::InboundEvent;
use crate::channels::model::message::ReplyContext;
use crate::channels::runtime::channel_trait::ChannelOutbound;
use crate::channels::runtime::channel_trait::ChannelPlugin;
use crate::channels::runtime::channel_trait::InboundKind;
use crate::channels::runtime::channel_trait::WebhookHandler;
use crate::channels::runtime::diagnostics;
use crate::types::ErrorCode;
use crate::types::Result;

pub const GITHUB_CHANNEL_TYPE: &str = "github";
const GITHUB_API: &str = "https://api.github.com";
const GITHUB_MAX_COMMENT_LEN: usize = 65_536;

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    pub token: String,
    #[serde(default)]
    pub webhook_secret: String,
}

// ── Plugin ──

pub struct GitHubChannel {
    client: reqwest::Client,
}

impl GitHubChannel {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("bendclaw")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for GitHubChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelPlugin for GitHubChannel {
    fn channel_type(&self) -> &str {
        GITHUB_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::EventDriven,
            inbound_mode: InboundMode::Webhook,
            supports_edit: false,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: true,
            supports_reactions: true,
            max_message_len: GITHUB_MAX_COMMENT_LEN,
            stale_event_threshold: None,
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let c: GitHubConfig = serde_json::from_value(config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid github config: {e}")))?;
        if c.token.is_empty() {
            return Err(ErrorCode::config("github token is required"));
        }
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(GitHubOutbound {
            client: self.client.clone(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Webhook(Arc::new(GitHubWebhookHandler))
    }
}

// ── Outbound ──

struct GitHubOutbound {
    client: reqwest::Client,
}

impl GitHubOutbound {
    fn api_url(path: &str) -> String {
        format!("{GITHUB_API}{path}")
    }

    fn extract_token(config: &serde_json::Value) -> Result<String> {
        config
            .get("token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| ErrorCode::config("github config missing token"))
    }
}

#[async_trait]
impl ChannelOutbound for GitHubOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&format!("/{chat_id}/comments"));
        let body = serde_json::json!({ "body": text });
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("github comment: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("github comment response: {e}")))?;
        let comment_id = json["id"]
            .as_i64()
            .ok_or_else(|| {
                ErrorCode::internal(format!("github comment: missing id in response: {json}"))
            })?
            .to_string();
        Ok(comment_id)
    }

    async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _text: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "github channel does not support edit_message",
        ))
    }

    async fn add_reaction(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        emoji: &str,
    ) -> Result<()> {
        let token = Self::extract_token(config)?;
        let url = Self::api_url(&format!("/{chat_id}/comments/{msg_id}/reactions"));
        let body = serde_json::json!({ "content": emoji });
        self.client
            .post(&url)
            .bearer_auth(&token)
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("github reaction: {e}")))?;
        diagnostics::log_channel_sent_github_reaction(chat_id, msg_id, emoji);
        Ok(())
    }
}

// ── Webhook Handler ──

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubWebhookPayload {
    action: Option<String>,
    issue: Option<GitHubIssue>,
    pull_request: Option<GitHubPullRequest>,
    comment: Option<GitHubComment>,
    repository: Option<GitHubRepo>,
    sender: Option<GitHubUser>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubIssue {
    number: u64,
    title: String,
    body: Option<String>,
    user: Option<GitHubUser>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubPullRequest {
    number: u64,
    title: String,
    body: Option<String>,
    user: Option<GitHubUser>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubComment {
    id: u64,
    body: Option<String>,
    user: Option<GitHubUser>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubRepo {
    full_name: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubUser {
    login: String,
}

pub struct GitHubWebhookHandler;

impl WebhookHandler for GitHubWebhookHandler {
    fn verify(&self, _external_account_id: &str, _headers: &HeaderMap, body: &[u8]) -> Result<()> {
        let _: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| ErrorCode::invalid_input(format!("invalid github payload: {e}")))?;
        Ok(())
    }

    fn parse(&self, _external_account_id: &str, body: &[u8]) -> Result<Vec<InboundEvent>> {
        let payload: GitHubWebhookPayload = serde_json::from_slice(body)
            .map_err(|e| ErrorCode::invalid_input(format!("invalid github event: {e}")))?;

        let repo_name = payload
            .repository
            .as_ref()
            .map(|r| r.full_name.as_str())
            .unwrap_or("unknown");

        let action = payload.action.as_deref().unwrap_or("");

        if let Some(pr) = &payload.pull_request {
            let event_type = format!("pull_request.{action}");
            let chat_id = format!("repos/{repo_name}/issues/{}", pr.number);
            let summary = serde_json::json!({
                "number": pr.number,
                "title": pr.title,
                "body": pr.body,
                "author": pr.user.as_ref().map(|u| &u.login),
            });
            return Ok(vec![InboundEvent::PlatformEvent {
                event_type,
                payload: summary,
                reply_context: Some(ReplyContext {
                    chat_id,
                    reply_to_message_id: None,
                    thread_id: Some(pr.number.to_string()),
                }),
            }]);
        }

        if let Some(comment) = &payload.comment {
            if let Some(issue) = &payload.issue {
                let event_type = format!("issue_comment.{action}");
                let chat_id = format!("repos/{repo_name}/issues/{}", issue.number);
                let summary = serde_json::json!({
                    "issue_number": issue.number,
                    "issue_title": issue.title,
                    "comment_id": comment.id,
                    "comment_body": comment.body,
                    "author": comment.user.as_ref().map(|u| &u.login),
                });
                return Ok(vec![InboundEvent::PlatformEvent {
                    event_type,
                    payload: summary,
                    reply_context: Some(ReplyContext {
                        chat_id,
                        reply_to_message_id: Some(comment.id.to_string()),
                        thread_id: Some(issue.number.to_string()),
                    }),
                }]);
            }
        }

        if let Some(issue) = &payload.issue {
            let event_type = format!("issues.{action}");
            let chat_id = format!("repos/{repo_name}/issues/{}", issue.number);
            let summary = serde_json::json!({
                "number": issue.number,
                "title": issue.title,
                "body": issue.body,
                "author": issue.user.as_ref().map(|u| &u.login),
            });
            return Ok(vec![InboundEvent::PlatformEvent {
                event_type,
                payload: summary,
                reply_context: Some(ReplyContext {
                    chat_id,
                    reply_to_message_id: None,
                    thread_id: Some(issue.number.to_string()),
                }),
            }]);
        }

        Ok(vec![])
    }
}
