//! Feishu outbound message sending.

use async_trait::async_trait;

use crate::base::{ErrorCode, Result};
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::observability::log::slog;

use super::config::FeishuConfig;
use super::token::{get_token, is_token_error, TokenCache};

const FEISHU_API: &str = "https://open.feishu.cn/open-apis";

pub struct FeishuOutbound {
    pub(super) client: reqwest::Client,
    pub(super) token_cache: TokenCache,
}

impl FeishuOutbound {
    fn parse_config(config: &serde_json::Value) -> Result<FeishuConfig> {
        FeishuConfig::from_json(config)
    }

    async fn token(&self, config: &FeishuConfig) -> Result<String> {
        get_token(&self.client, FEISHU_API, &config.app_id, &config.app_secret, &self.token_cache)
            .await
    }

    /// POST/PUT with automatic token retry on 401 or business code 99991663.
    async fn send_with_retry(
        &self,
        config: &FeishuConfig,
        build_req: impl Fn(&str) -> reqwest::RequestBuilder,
    ) -> Result<serde_json::Value> {
        let token = self.token(config).await?;
        let resp = build_req(&token)
            .send()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu http: {e}")))?;
        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ErrorCode::internal(format!("feishu http response: {e}")))?;

        if is_token_error(status, &body) {
            self.token_cache.invalidate().await;
            let new_token = self.token(config).await?;
            let resp2 = build_req(&new_token)
                .send()
                .await
                .map_err(|e| ErrorCode::internal(format!("feishu http retry: {e}")))?;
            let body2: serde_json::Value = resp2
                .json()
                .await
                .map_err(|e| ErrorCode::internal(format!("feishu http retry response: {e}")))?;
            return Ok(body2);
        }

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
        let cfg = Self::parse_config(config)?;
        let url = format!("{FEISHU_API}/im/v1/messages?receive_id_type=chat_id");
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content,
        });

        let client = self.client.clone();
        let body_clone = body.clone();
        let resp = self
            .send_with_retry(&cfg, |token| {
                client.post(&url).bearer_auth(token).json(&body_clone)
            })
            .await?;

        let msg_id = resp["data"]["message_id"].as_str().unwrap_or("").to_string();
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
        let cfg = Self::parse_config(config)?;
        let url = format!("{FEISHU_API}/im/v1/messages/{msg_id}");
        let content = serde_json::json!({ "text": text }).to_string();
        let body = serde_json::json!({ "msg_type": "text", "content": content });

        let client = self.client.clone();
        let body_clone = body.clone();
        let resp = self
            .send_with_retry(&cfg, |token| {
                client.put(&url).bearer_auth(token).json(&body_clone)
            })
            .await?;

        let code = resp["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let msg = resp["msg"].as_str().unwrap_or("unknown");
            slog!(warn, "feishu_outbound", "edit_failed", msg_id, code, msg,);
            return Err(ErrorCode::channel_send(format!(
                "feishu edit_message failed: code={code}, msg={msg}"
            )));
        }
        Ok(())
    }

    async fn add_reaction(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal("feishu channel does not support reactions via outbound"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_outbound() -> FeishuOutbound {
        FeishuOutbound {
            client: reqwest::Client::new(),
            token_cache: TokenCache::new(),
        }
    }

    #[tokio::test]
    async fn add_reaction_returns_error() {
        let o = make_outbound();
        assert!(o
            .add_reaction(&serde_json::json!({}), "chat", "msg", "thumbsup")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn send_text_invalid_config_returns_error() {
        let o = make_outbound();
        assert!(o.send_text(&serde_json::json!({}), "chat", "hello").await.is_err());
    }

    #[tokio::test]
    async fn edit_message_invalid_config_returns_error() {
        let o = make_outbound();
        assert!(o
            .edit_message(&serde_json::json!({}), "chat", "msg_1", "text")
            .await
            .is_err());
    }
}
