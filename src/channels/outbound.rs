use std::time::Instant;

use super::ChannelRegistry;
use crate::channels::runtime::diagnostics;
use crate::storage::dal::channel_account::record::ChannelAccountRecord;
use crate::types::truncate_bytes_on_char_boundary;
use crate::types::ErrorCode;
use crate::types::Result;

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
        Ok(message_id) => diagnostics::log_channel_sent(
            payload.len(),
            started.elapsed().as_millis() as u64,
            &account.channel_type,
            &account.id,
            &account.account_id,
            chat_id,
            "text",
            message_id,
        ),
        Err(error) => diagnostics::log_channel_failed(
            payload.len(),
            started.elapsed().as_millis() as u64,
            error,
            &account.channel_type,
            &account.id,
            &account.account_id,
            chat_id,
            "text",
        ),
    }
    result
}
