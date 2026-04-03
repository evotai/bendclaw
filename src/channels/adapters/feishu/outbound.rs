use std::sync::Arc;

use async_trait::async_trait;

use super::config::FEISHU_API;
use super::token::get_token;
use super::token::is_token_error;
use super::token::TokenCache;
use crate::channels::runtime::channel_trait::ChannelOutbound;
use crate::channels::runtime::diagnostics;
use crate::types::ErrorCode;
use crate::types::Result;

// ── Outbound ──

pub struct FeishuOutbound {
    pub(super) client: reqwest::Client,
    pub(super) token_cache: Arc<TokenCache>,
}

impl FeishuOutbound {
    fn check_api_error(body: &serde_json::Value) -> Result<()> {
        let code = body["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let msg = body["msg"].as_str().unwrap_or("unknown");
            diagnostics::log_feishu_send_failed(code, msg);
            return Err(ErrorCode::channel_send(format!(
                "feishu API error: code={code}, msg={msg}"
            )));
        }
        Ok(())
    }

    fn extract_credentials(config: &serde_json::Value) -> Result<(String, String)> {
        let app_id = config
            .get("app_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_id"))?;
        let app_secret = config
            .get("app_secret")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ErrorCode::config("feishu config missing app_secret"))?;
        Ok((app_id.to_string(), app_secret.to_string()))
    }

    /// Send an HTTP request with token, retrying once on token error.
    async fn send_with_retry<F, Fut>(
        &self,
        config: &serde_json::Value,
        op: F,
    ) -> Result<serde_json::Value>
    where
        F: Fn(reqwest::Client, String) -> Fut,
        Fut: std::future::Future<Output = Result<(u16, serde_json::Value)>>,
    {
        let (app_id, app_secret) = Self::extract_credentials(config)?;
        let token = get_token(&self.client, &app_id, &app_secret, &self.token_cache).await?;

        let (status, body) = op(self.client.clone(), token).await?;
        if is_token_error(status, &body) {
            self.token_cache.invalidate().await;
            let token2 = get_token(&self.client, &app_id, &app_secret, &self.token_cache).await?;
            let (status2, body2) = op(self.client.clone(), token2).await?;
            if is_token_error(status2, &body2) {
                return Err(ErrorCode::internal(format!(
                    "feishu token retry failed: HTTP {status2}"
                )));
            }
            Self::check_api_error(&body2)?;
            return Ok(body2);
        }
        Self::check_api_error(&body)?;
        Ok(body)
    }
}

#[async_trait]
impl ChannelOutbound for FeishuOutbound {
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String> {
        let chat_id = chat_id.to_string();
        let text = text.to_string();
        let json = self
            .send_with_retry(config, |client, token| {
                let chat_id = chat_id.clone();
                let text = text.clone();
                async move {
                    let url = format!("{FEISHU_API}/im/v1/messages?receive_id_type=chat_id");
                    let content = serde_json::json!({ "text": text }).to_string();
                    let body = serde_json::json!({
                        "receive_id": chat_id,
                        "msg_type": "text",
                        "content": content,
                    });
                    let resp = client
                        .post(&url)
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| ErrorCode::internal(format!("feishu send: {e}")))?;
                    let status = resp.status().as_u16();
                    let json: serde_json::Value = resp
                        .json()
                        .await
                        .map_err(|e| ErrorCode::internal(format!("feishu send response: {e}")))?;
                    Ok((status, json))
                }
            })
            .await?;

        let msg_id = json["data"]["message_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        Ok(msg_id)
    }

    async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(
        &self,
        config: &serde_json::Value,
        _chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        let msg_id = msg_id.to_string();
        let text = text.to_string();
        let json = self
            .send_with_retry(config, |client, token| {
                let msg_id = msg_id.clone();
                let text = text.clone();
                async move {
                    let url = format!("{FEISHU_API}/im/v1/messages/{msg_id}");
                    let content = serde_json::json!({ "text": text }).to_string();
                    let body = serde_json::json!({
                        "msg_type": "text",
                        "content": content,
                    });
                    let resp = client
                        .put(&url)
                        .bearer_auth(&token)
                        .json(&body)
                        .send()
                        .await
                        .map_err(|e| ErrorCode::internal(format!("feishu edit_message: {e}")))?;
                    let status = resp.status().as_u16();
                    if !reqwest::StatusCode::from_u16(status)
                        .map(|s| s.is_success())
                        .unwrap_or(false)
                    {
                        let body_text = resp.text().await.unwrap_or_default();
                        diagnostics::log_feishu_edit_failed(status, &body_text);
                        return Err(ErrorCode::channel_send(format!(
                            "feishu edit_message failed: HTTP {status}: {body_text}"
                        )));
                    }
                    let json: serde_json::Value = resp
                        .json()
                        .await
                        .map_err(|e| ErrorCode::internal(format!("feishu edit response: {e}")))?;
                    Ok((status, json))
                }
            })
            .await?;
        let _ = json;
        Ok(())
    }

    async fn add_reaction(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "feishu channel does not support reactions",
        ))
    }

    async fn update_draft(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()> {
        self.edit_message(config, chat_id, msg_id, text).await
    }
}

// ── Reaction helper (fire-and-forget from WS loop) ──

pub async fn add_reaction(
    client: &reqwest::Client,
    token_cache: &TokenCache,
    app_id: &str,
    app_secret: &str,
    message_id: &str,
    emoji_type: &str,
) {
    let token = match get_token(client, app_id, app_secret, token_cache).await {
        Ok(t) => t,
        Err(e) => {
            diagnostics::log_feishu_reaction_token_failed(&e);
            return;
        }
    };
    let url = format!("{FEISHU_API}/im/v1/messages/{message_id}/reactions");
    let body = serde_json::json!({
        "reaction_type": { "emoji_type": emoji_type }
    });
    match client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                diagnostics::log_feishu_reaction_sent(message_id, emoji_type);
            } else {
                let body = resp.text().await.unwrap_or_default();
                diagnostics::log_feishu_reaction_failed(status, &body, message_id, emoji_type);
            }
        }
        Err(e) => {
            diagnostics::log_feishu_reaction_request_failed(&e, message_id);
        }
    }
}
