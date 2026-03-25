use std::sync::Arc;

use crate::base::new_id;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::context::ChannelContext;
use crate::kernel::channel::debouncer::DebouncedInput;
use crate::kernel::channel::delivery;
use crate::kernel::channel::dispatcher::ChannelDispatcher;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::channel::typing_keepalive::TypingKeepalive;
use crate::kernel::channel::typing_keepalive::TypingKeepaliveConfig;
use crate::kernel::channel::writer::ChannelMessageOp;
use crate::kernel::runtime::Runtime;
use crate::kernel::runtime::SubmitResult;
use crate::observability::log::channel_log;
use crate::observability::log::slog;
use crate::storage::dal::channel_message::record::ChannelMessageRecord;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

/// Dispatch a debounced input through the full conversation pipeline.
/// Called by ChatRouter after per-chat serialization and debounce.
pub async fn dispatch_debounced(runtime: &Arc<Runtime>, input: DebouncedInput) {
    let account = input.account.clone();
    if let Err(e) = try_dispatch_debounced(runtime, input).await {
        slog!(error, "channel", "dispatch_failed",
            agent_id = %account.agent_id,
            channel_type = %account.channel_type,
            account_id = %account.channel_account_id,
            error = %e,
        );
    }
}

// ── Pipeline stages ─────────────────────────────────────────────────────────

/// Stage 1: Extract input and validate sender trust.
struct ValidatedInput {
    text: String,
    chat_id: String,
}

fn extract_and_validate(account: &ChannelAccount, event: &InboundEvent) -> Option<ValidatedInput> {
    let (text, reply_ctx) = ChannelDispatcher::extract_input(event);

    // Sender trust check.
    if let Some(sender_id) = event_sender_id(event) {
        if !is_sender_allowed(&account.config, sender_id) {
            return None;
        }
    }

    if text.trim().is_empty() {
        return None;
    }

    let chat_id = reply_ctx
        .as_ref()
        .map(|r| r.chat_id.clone())
        .unwrap_or_default();

    Some(ValidatedInput { text, chat_id })
}

/// Stage 2: Resolve dispatch context (session, outbound, repo, capabilities).
struct DispatchContext {
    session_id: String,
    base_key: String,
    outbound: Option<Arc<dyn ChannelOutbound>>,
    msg_repo: ChannelMessageRepo,
    max_message_len: usize,
}

async fn resolve_dispatch_context(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    chat_id: &str,
) -> std::result::Result<DispatchContext, Box<dyn std::error::Error + Send + Sync>> {
    let base_key =
        ChannelContext::base_key(&account.channel_type, &account.external_account_id, chat_id);

    let outbound = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.outbound());

    let session_id = runtime
        .session_lifecycle()
        .resolve_active(&account.agent_id, &account.user_id, &base_key)
        .await?
        .id;

    let pool = runtime.databases().agent_pool(&account.agent_id)?;
    let msg_repo = ChannelMessageRepo::new(pool.clone());

    let caps = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.capabilities());
    let max_message_len = caps.as_ref().map(|c| c.max_message_len).unwrap_or(4096);

    Ok(DispatchContext {
        session_id,
        base_key,
        outbound,
        msg_repo,
        max_message_len,
    })
}

/// Stage 3: Handle slash commands (/new, /clear). Returns true if handled.
async fn handle_slash_command(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    ctx: &mut DispatchContext,
    chat_id: &str,
    input: &str,
) -> bool {
    let trimmed = input.trim();
    if trimmed != "/new" && trimmed != "/clear" {
        return false;
    }

    let old_session_id = ctx.session_id.clone();
    let new_session = match runtime
        .session_lifecycle()
        .start_new(
            &account.agent_id,
            &account.user_id,
            &ctx.base_key,
            trimmed.trim_start_matches('/'),
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            slog!(warn, "channel", "session_reset_failed",
                agent_id = %account.agent_id,
                channel_type = %account.channel_type,
                account_id = %account.channel_account_id,
                chat_id,
                error = %error,
            );
            if let Some(ref ob) = ctx.outbound {
                let _ = ob
                    .send_text(
                        &account.config,
                        chat_id,
                        "Failed to start a new conversation.",
                    )
                    .await;
            }
            return true;
        }
    };
    ctx.session_id = new_session.id.clone();

    channel_log!(info, "command", "new_session",
        channel_type = %account.channel_type,
        account_id = %account.channel_account_id,
        chat_id,
        old_session = %old_session_id,
        new_session = %new_session.id,
    );

    if let Some(ref ob) = ctx.outbound {
        let _ = ob
            .send_text(&account.config, chat_id, "New conversation started.")
            .await;
    }

    true
}

/// Stage 4: Dedup check. Returns true if ANY event in the batch is a duplicate.
async fn is_duplicate(
    msg_repo: &ChannelMessageRepo,
    account: &ChannelAccount,
    events: &[InboundEvent],
) -> bool {
    for event in events {
        if let InboundEvent::Message(msg) = event {
            if !msg.message_id.is_empty()
                && msg_repo
                    .exists_by_platform_message_id(
                        &account.channel_type,
                        &account.external_account_id,
                        &msg.chat_id,
                        &msg.message_id,
                    )
                    .await
                    .unwrap_or_else(|e| {
                        slog!(warn, "channel", "dedup_check_failed",
                            message_id = %msg.message_id,
                            channel_type = %account.channel_type,
                            error = %e,
                        );
                        false
                    })
            {
                return true;
            }
        }
    }
    false
}

/// Record all inbound messages (fire-and-forget).
fn record_inbound(
    runtime: &Runtime,
    msg_repo: &ChannelMessageRepo,
    account: &ChannelAccount,
    session_id: &str,
    events: &[InboundEvent],
) {
    for event in events {
        if let InboundEvent::Message(msg) = event {
            runtime
                .channel_message_writer
                .send(ChannelMessageOp::Insert {
                    repo: msg_repo.clone(),
                    record: ChannelMessageRecord {
                        id: new_id(),
                        channel_type: account.channel_type.clone(),
                        account_id: account.external_account_id.clone(),
                        chat_id: msg.chat_id.clone(),
                        session_id: session_id.to_string(),
                        direction: "inbound".into(),
                        sender_id: msg.sender_id.clone(),
                        text: msg.text.clone(),
                        platform_message_id: msg.message_id.clone(),
                        run_id: String::new(),
                        attachments: "[]".into(),
                        created_at: String::new(),
                    },
                });
        }
    }
}

/// Stage 5: Submit turn and deliver response (with followup loop).
#[allow(clippy::too_many_arguments)]
async fn submit_and_deliver(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    ctx: &DispatchContext,
    chat_id: &str,
    input: &str,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut pending_input = Some(input.to_string());

    loop {
        let trace_id = new_id();
        let submit = runtime
            .submit_turn(
                &account.agent_id,
                &ctx.session_id,
                &account.user_id,
                &pending_input.take().unwrap_or_default(),
                &trace_id,
                None,
                "",
                "",
                false,
            )
            .await?;

        match submit {
            SubmitResult::Control { message } => {
                send_control_reply(
                    runtime,
                    ctx.outbound.as_ref(),
                    &ctx.msg_repo,
                    account,
                    &ctx.session_id,
                    chat_id,
                    &message,
                )
                .await;
                break;
            }
            SubmitResult::Injected | SubmitResult::Queued => {
                break;
            }
            SubmitResult::Started { stream, preamble } => {
                if let Some(ref text) = preamble {
                    if let Some(ref ob) = ctx.outbound {
                        let _ = ob.send_text(&account.config, chat_id, text).await;
                    }
                }

                let run_id = stream.run_id().to_string();
                let (output_text, platform_msg_id) = if let Some(ref ob) = ctx.outbound {
                    let result = delivery::deliver_outbound(
                        ob,
                        &runtime.rate_limiter,
                        &account.channel_type,
                        &account.external_account_id,
                        &account.config,
                        chat_id,
                        ctx.max_message_len,
                        stream,
                    )
                    .await?;
                    match result {
                        Some(r) => (r.text, r.platform_message_id),
                        None => {
                            if let Some(next) = take_followup(runtime, &ctx.session_id) {
                                pending_input = Some(next);
                                continue;
                            }
                            break;
                        }
                    }
                } else {
                    let _ = stream.finish_output().await?;
                    if let Some(next) = take_followup(runtime, &ctx.session_id) {
                        pending_input = Some(next);
                        continue;
                    }
                    break;
                };

                channel_log!(info, "outbound", "sent",
                    msg = format!("channel \u{2192} {}", account.channel_type),
                    output_preview = %crate::base::truncate_bytes_on_char_boundary(&output_text, 100),
                    output_bytes = output_text.len(),
                    channel_type = %account.channel_type,
                    account_id = %account.channel_account_id,
                    chat_id,
                    message_id = %platform_msg_id,
                );

                runtime
                    .channel_message_writer
                    .send(ChannelMessageOp::Insert {
                        repo: ctx.msg_repo.clone(),
                        record: ChannelMessageRecord {
                            id: new_id(),
                            channel_type: account.channel_type.clone(),
                            account_id: account.external_account_id.clone(),
                            chat_id: chat_id.to_string(),
                            session_id: ctx.session_id.clone(),
                            direction: "outbound".into(),
                            sender_id: "agent".into(),
                            text: output_text,
                            platform_message_id: platform_msg_id,
                            run_id: run_id.clone(),
                            attachments: "[]".into(),
                            created_at: String::new(),
                        },
                    });

                if let Some(next) = take_followup(runtime, &ctx.session_id) {
                    pending_input = Some(next);
                    continue;
                }
                break;
            }
        }
    }

    Ok(())
}

fn take_followup(runtime: &Runtime, session_id: &str) -> Option<String> {
    runtime
        .sessions()
        .get(session_id)
        .and_then(|session| session.take_followup())
}

// ── Orchestrator ────────────────────────────────────────────────────────────

async fn try_dispatch_debounced(
    runtime: &Arc<Runtime>,
    input: DebouncedInput,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let account = &input.account;
    let validated = match extract_and_validate(account, &input.primary_event) {
        Some(v) => v,
        None => return Ok(()),
    };

    // Use debounced text if messages were merged, otherwise use extracted text.
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
        slog!(info, "channel", "dedup_skipped",
            channel_type = %account.channel_type,
        );
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

    // Start typing keepalive for the full dispatch lifecycle.
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

// ── Helpers ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn send_control_reply(
    runtime: &Arc<Runtime>,
    outbound: Option<&Arc<dyn ChannelOutbound>>,
    msg_repo: &ChannelMessageRepo,
    account: &ChannelAccount,
    session_id: &str,
    chat_id: &str,
    message: &str,
) {
    if let Some(ob) = outbound {
        match ob.send_text(&account.config, chat_id, message).await {
            Ok(platform_message_id) => {
                channel_log!(info, "outbound", "sent",
                    msg = format!("channel \u{2192} {} (control)", account.channel_type),
                    output_preview = %crate::base::truncate_bytes_on_char_boundary(message, 100),
                    output_bytes = message.len(),
                    channel_type = %account.channel_type,
                    account_id = %account.channel_account_id,
                    chat_id,
                    message_id = %platform_message_id,
                );
                runtime
                    .channel_message_writer
                    .send(ChannelMessageOp::Insert {
                        repo: msg_repo.clone(),
                        record: ChannelMessageRecord {
                            id: new_id(),
                            channel_type: account.channel_type.clone(),
                            account_id: account.external_account_id.clone(),
                            chat_id: chat_id.to_string(),
                            session_id: session_id.to_string(),
                            direction: "outbound".into(),
                            sender_id: "agent".into(),
                            text: message.to_string(),
                            platform_message_id,
                            run_id: String::new(),
                            attachments: "[]".into(),
                            created_at: String::new(),
                        },
                    });
            }
            Err(error) => {
                channel_log!(warn, "outbound", "failed",
                    msg = format!("channel \u{2192} {} (control)", account.channel_type),
                    output_preview = %crate::base::truncate_bytes_on_char_boundary(message, 100),
                    output_bytes = message.len(),
                    channel_type = %account.channel_type,
                    account_id = %account.channel_account_id,
                    chat_id,
                    error = %error,
                );
            }
        }
    }
}

/// Extract sender_id from any inbound event variant.
fn event_sender_id(event: &InboundEvent) -> Option<&str> {
    match event {
        InboundEvent::Message(msg) if !msg.sender_id.is_empty() => Some(&msg.sender_id),
        _ => None,
    }
}

fn event_message_id(event: &InboundEvent) -> Option<&str> {
    match event {
        InboundEvent::Message(msg) if !msg.message_id.is_empty() => Some(&msg.message_id),
        _ => None,
    }
}

/// Check if a sender is allowed by the account config's `allow_from` list.
/// - Missing or empty `allow_from` → allow all (backward compatible).
/// - `"*"` in the list → allow all.
/// - Otherwise sender_id must match one of the entries.
pub fn is_sender_allowed(config: &serde_json::Value, sender_id: &str) -> bool {
    let Some(list) = config.get("allow_from").and_then(|v| v.as_array()) else {
        return true; // no allow_from configured → allow all
    };
    if list.is_empty() {
        return true;
    }
    list.iter().any(|entry| {
        let s = entry.as_str().unwrap_or("");
        s == "*" || s == sender_id || s.split('|').any(|part| part == sender_id)
    })
}
