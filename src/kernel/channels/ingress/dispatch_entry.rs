use std::sync::Arc;

use super::dedup::is_duplicate;
use super::dispatch_context::resolve_dispatch_context;
use super::inbound_recorder::record_inbound;
use super::input_validation::event_message_id;
use super::input_validation::event_sender_id;
use super::input_validation::extract_and_validate;
use super::slash_command::handle_slash_command;
use super::submit_and_deliver::submit_and_deliver;
use crate::kernel::channels::routing::debouncer::DebouncedInput;
use crate::kernel::channels::routing::typing_keepalive::TypingKeepalive;
use crate::kernel::channels::routing::typing_keepalive::TypingKeepaliveConfig;
use crate::kernel::channels::runtime::diagnostics;
use crate::kernel::runtime::Runtime;
use crate::observability::log::channel_log;

/// Dispatch a debounced input through the full conversation pipeline.
/// Called by ChatRouter after per-chat serialization and debounce.
pub async fn dispatch_debounced(runtime: &Arc<Runtime>, input: DebouncedInput) {
    let account = input.account.clone();
    if let Err(e) = try_dispatch_debounced(runtime, input).await {
        diagnostics::log_channel_dispatch_failed(
            &account.agent_id,
            &account.channel_type,
            &account.channel_account_id,
            &e,
        );
    }
}

async fn try_dispatch_debounced(
    runtime: &Arc<Runtime>,
    input: DebouncedInput,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = &input.account;
    let validated = match extract_and_validate(account, &input.primary_event) {
        Some(v) => v,
        None => return Ok(()),
    };

    let text = if input.merged_count > 1 {
        &input.text
    } else {
        &validated.text
    };

    let mut ctx = resolve_dispatch_context(runtime, account, &validated.chat_id).await?;

    if handle_slash_command(runtime, account, &mut ctx, &validated.chat_id, text).await {
        return Ok(());
    }

    if is_duplicate(&ctx.msg_repo, account, &input.all_events).await {
        diagnostics::log_channel_dedup_skipped(&account.channel_type);
        return Ok(());
    }

    channel_log!(info, "inbound", "accepted",
        msg = format!("channel \u{2190} {}", account.channel_type),
        input_preview = %crate::base::truncate_bytes_on_char_boundary(text, 100),
        input_bytes = text.len(),
        channel_type = %account.channel_type,
        account_id = %account.channel_account_id,
        chat_id = %validated.chat_id,
        sender_id = event_sender_id(&input.primary_event).unwrap_or(""),
        message_id = event_message_id(&input.primary_event).unwrap_or(""),
        merged_count = input.merged_count,
    );

    record_inbound(
        runtime,
        &ctx.msg_repo,
        account,
        &ctx.session_id,
        &input.all_events,
    );

    let typing = ctx.outbound.as_ref().map(|ob| {
        TypingKeepalive::start(
            ob.clone(),
            account.config.clone(),
            validated.chat_id.clone(),
            TypingKeepaliveConfig::default(),
        )
    });

    let result = submit_and_deliver(runtime, account, &ctx, &validated.chat_id, text).await;

    if let Some(t) = typing {
        t.stop();
    }

    result
}
