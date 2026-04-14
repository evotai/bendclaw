use super::config::FEISHU_API;
use super::token::get_token;
use super::token::is_token_error;
use super::token::TokenCache;
use crate::error::EvotError;
use crate::error::Result;

/// Send a text message to a Feishu chat, with automatic token retry.
pub async fn send_text(
    client: &reqwest::Client,
    token_cache: &TokenCache,
    app_id: &str,
    app_secret: &str,
    chat_id: &str,
    text: &str,
) -> Result<String> {
    let url = format!("{FEISHU_API}/im/v1/messages?receive_id_type=chat_id");
    let content = serde_json::json!({ "text": text }).to_string();
    let body = serde_json::json!({
        "receive_id": chat_id,
        "msg_type": "text",
        "content": content,
    });

    let token = get_token(client, app_id, app_secret, token_cache).await?;
    let resp = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .map_err(|e| EvotError::Run(format!("feishu send: {e}")))?;

    let status = resp.status().as_u16();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| EvotError::Run(format!("feishu send response: {e}")))?;

    // Retry once on token error
    if is_token_error(status, &json) {
        token_cache.invalidate().await;
        let token2 = get_token(client, app_id, app_secret, token_cache).await?;
        let resp2 = client
            .post(&url)
            .bearer_auth(&token2)
            .json(&body)
            .send()
            .await
            .map_err(|e| EvotError::Run(format!("feishu send retry: {e}")))?;
        let status2 = resp2.status().as_u16();
        let json2: serde_json::Value = resp2
            .json()
            .await
            .map_err(|e| EvotError::Run(format!("feishu send retry response: {e}")))?;
        if is_token_error(status2, &json2) {
            return Err(EvotError::Run(format!(
                "feishu token retry failed: HTTP {status2}"
            )));
        }
        check_api_error(&json2)?;
        return Ok(extract_message_id(&json2));
    }

    check_api_error(&json)?;
    Ok(extract_message_id(&json))
}

/// Add a reaction (emoji) to a message. Fire-and-forget, logs errors.
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
            tracing::warn!(channel = "feishu", error = %e, "failed to get token for reaction");
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
            if !resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                tracing::warn!(
                    channel = "feishu",
                    message_id,
                    emoji_type,
                    body,
                    "reaction failed"
                );
            }
        }
        Err(e) => {
            tracing::warn!(channel = "feishu", error = %e, message_id, "reaction request failed");
        }
    }
}

fn check_api_error(body: &serde_json::Value) -> Result<()> {
    let code = body["code"].as_i64().unwrap_or(0);
    if code != 0 {
        let msg = body["msg"].as_str().unwrap_or("unknown");
        return Err(EvotError::Run(format!(
            "feishu API error: code={code}, msg={msg}"
        )));
    }
    Ok(())
}

fn extract_message_id(json: &serde_json::Value) -> String {
    json["data"]["message_id"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}
