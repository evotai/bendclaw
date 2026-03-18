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

    outbound.send_text(&account.config, chat_id, &payload).await
}
