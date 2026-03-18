use std::time::Instant;

use super::ChannelRegistry;
use crate::base::truncate_bytes_on_char_boundary;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::ChannelAccountRecord;

pub async fn send_text_to_account(
    channels: &ChannelRegistry,
    account: &ChannelAccountRecord,
    chat_id: &str,
    text: &str,
) -> Result<String> {
    if !account.enabled {
        return Err(ErrorCode::invalid_input(format!(
            "channel account '{}' is disabled",
            account.id
        )));
    }

    let entry = channels.get(&account.channel_type).ok_or_else(|| {
        ErrorCode::not_found(format!(
            "channel plugin '{}' not registered",
            account.channel_type
        ))
    })?;

    let outbound = entry.plugin.outbound();
    let max_len = entry.plugin.capabilities().max_message_len;
    let mut payload = text.to_string();
    if payload.len() > max_len {
        payload = truncate_bytes_on_char_boundary(&payload, max_len);
    }

    let started = Instant::now();
    let result = outbound.send_text(&account.config, chat_id, &payload).await;
    match &result {
        Ok(message_id) => tracing::info!(
            channel_type = %account.channel_type,
            account_id = %account.id,
            external_account_id = %account.account_id,
            chat_id,
            send_type = "text",
            message_id,
            output_bytes = payload.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "channel outbound sent"
        ),
        Err(error) => tracing::warn!(
            channel_type = %account.channel_type,
            account_id = %account.id,
            external_account_id = %account.account_id,
            chat_id,
            send_type = "text",
            output_bytes = payload.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            error = %error,
            "channel outbound failed"
        ),
    }
    result
}
