use std::sync::Arc;

use super::dispatch_context::DispatchContext;
use crate::channels::egress::deliver_outbound;
use crate::channels::model::account::ChannelAccount;
use crate::channels::runtime::channel_trait::ChannelOutbound;
use crate::channels::runtime::writer::ChannelMessageOp;
use crate::kernel::runtime::Runtime;
use crate::kernel::runtime::SubmitResult;
use crate::observability::log::channel_log;
use crate::storage::dal::channel_message::record::ChannelMessageRecord;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;
use crate::types::new_id;

/// Submit turn and deliver response (with followup loop).
pub(crate) async fn submit_and_deliver(
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
                    let result = deliver_outbound(
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
                    output_preview = %crate::types::truncate_bytes_on_char_boundary(&output_text, 100),
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
                    output_preview = %crate::types::truncate_bytes_on_char_boundary(message, 100),
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
                    output_preview = %crate::types::truncate_bytes_on_char_boundary(message, 100),
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
