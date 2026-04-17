use std::time::Duration;

use tokio::time::Instant;

use super::traits::MessageSink;
use crate::agent::Run;
use crate::agent::RunEventPayload;
use crate::error::Result;

pub struct StreamDeliveryConfig {
    /// Minimum chars before sending the first message.
    pub min_initial_chars: usize,
    /// Minimum interval between edits.
    pub throttle: Duration,
    /// Show tool execution progress in the message.
    pub show_tool_progress: bool,
}

impl Default for StreamDeliveryConfig {
    fn default() -> Self {
        Self {
            min_initial_chars: 80,
            throttle: Duration::from_millis(1000),
            show_tool_progress: true,
        }
    }
}

/// Deliver a Run progressively through a MessageSink.
///
/// If the sink supports editing, sends an initial message then edits it
/// as new content arrives. Otherwise, waits for the stream to finish
/// and sends the final text in one shot.
pub async fn deliver(
    sink: &dyn MessageSink,
    chat_id: &str,
    run: &mut Run,
    config: &StreamDeliveryConfig,
) -> Result<String> {
    let caps = sink.capabilities();

    if caps.can_edit {
        deliver_progressive(sink, chat_id, run, config, caps.max_message_len).await
    } else {
        deliver_final(sink, chat_id, run, caps.max_message_len).await
    }
}

/// Progressive delivery: send first, then edit in-place.
async fn deliver_progressive(
    sink: &dyn MessageSink,
    chat_id: &str,
    run: &mut Run,
    config: &StreamDeliveryConfig,
    max_len: usize,
) -> Result<String> {
    let mut text_buf = String::new();
    let mut tool_status = String::new();
    let mut msg_id: Option<String> = None;
    let mut last_edit = Instant::now();
    let mut edit_broken = false;

    while let Some(event) = run.next().await {
        match &event.payload {
            RunEventPayload::AssistantDelta {
                delta: Some(delta), ..
            } if !delta.is_empty() => {
                text_buf.push_str(delta);

                if msg_id.is_none() && text_buf.len() >= config.min_initial_chars {
                    let display = compose_display(&text_buf, &tool_status, max_len);
                    match sink.send_text(chat_id, &display).await {
                        Ok(id) => {
                            msg_id = Some(id);
                            edit_broken = false;
                            last_edit = Instant::now();
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "delivery: send initial failed");
                        }
                    }
                } else if msg_id.is_some() && !edit_broken && last_edit.elapsed() >= config.throttle
                {
                    try_edit(
                        sink,
                        chat_id,
                        msg_id.as_deref(),
                        &text_buf,
                        &tool_status,
                        max_len,
                        &mut last_edit,
                        &mut edit_broken,
                    )
                    .await;

                    // Edit limit reached — send a new message and continue editing there
                    if edit_broken {
                        let display = compose_display(&text_buf, &tool_status, max_len);
                        if let Ok(id) = sink.send_text(chat_id, &display).await {
                            msg_id = Some(id);
                            edit_broken = false;
                            last_edit = Instant::now();
                        }
                    }
                }
            }

            RunEventPayload::ToolStarted { tool_name, .. } if config.show_tool_progress => {
                tool_status = format!("\u{1f527} {tool_name}...");
                if !edit_broken {
                    try_edit(
                        sink,
                        chat_id,
                        msg_id.as_deref(),
                        &text_buf,
                        &tool_status,
                        max_len,
                        &mut last_edit,
                        &mut edit_broken,
                    )
                    .await;
                    if edit_broken {
                        let display = compose_display(&text_buf, &tool_status, max_len);
                        if let Ok(id) = sink.send_text(chat_id, &display).await {
                            msg_id = Some(id);
                            edit_broken = false;
                            last_edit = Instant::now();
                        }
                    }
                }
            }

            RunEventPayload::ToolFinished {
                tool_name,
                is_error,
                ..
            } if config.show_tool_progress => {
                let icon = if *is_error { "\u{274c}" } else { "\u{2705}" };
                tool_status = format!("{icon} {tool_name}");
                if !edit_broken {
                    try_edit(
                        sink,
                        chat_id,
                        msg_id.as_deref(),
                        &text_buf,
                        &tool_status,
                        max_len,
                        &mut last_edit,
                        &mut edit_broken,
                    )
                    .await;
                    if edit_broken {
                        let display = compose_display(&text_buf, &tool_status, max_len);
                        if let Ok(id) = sink.send_text(chat_id, &display).await {
                            msg_id = Some(id);
                            edit_broken = false;
                            last_edit = Instant::now();
                        }
                    }
                }
            }

            RunEventPayload::ToolProgress { text, .. } if config.show_tool_progress => {
                tool_status = format!("\u{23f3} {text}");
                if !edit_broken {
                    try_edit(
                        sink,
                        chat_id,
                        msg_id.as_deref(),
                        &text_buf,
                        &tool_status,
                        max_len,
                        &mut last_edit,
                        &mut edit_broken,
                    )
                    .await;
                    if edit_broken {
                        let display = compose_display(&text_buf, &tool_status, max_len);
                        if let Ok(id) = sink.send_text(chat_id, &display).await {
                            msg_id = Some(id);
                            edit_broken = false;
                            last_edit = Instant::now();
                        }
                    }
                }
            }

            _ => {}
        }
    }

    // Final delivery
    let final_text = truncate_safe(&text_buf, max_len);
    if final_text.is_empty() {
        return Ok(text_buf);
    }

    match msg_id {
        Some(ref id) if !edit_broken => {
            if let Err(e) = sink.edit_text(chat_id, id, &final_text).await {
                tracing::warn!(error = %e, "delivery: final edit failed, sending new message");
                let _ = sink.send_text(chat_id, &final_text).await;
            }
        }
        _ => {
            let _ = sink.send_text(chat_id, &final_text).await;
        }
    }

    Ok(text_buf)
}

/// Non-edit delivery: collect everything, send once.
async fn deliver_final(
    sink: &dyn MessageSink,
    chat_id: &str,
    run: &mut Run,
    max_len: usize,
) -> Result<String> {
    let mut text_buf = String::new();
    while let Some(event) = run.next().await {
        if let RunEventPayload::AssistantDelta {
            delta: Some(delta), ..
        } = &event.payload
        {
            if !delta.is_empty() {
                text_buf.push_str(delta);
            }
        }
    }

    if !text_buf.is_empty() {
        let final_text = truncate_safe(&text_buf, max_len);
        let _ = sink.send_text(chat_id, &final_text).await;
    }

    Ok(text_buf)
}

// ── Helpers ──

#[allow(clippy::too_many_arguments)]
async fn try_edit(
    sink: &dyn MessageSink,
    chat_id: &str,
    msg_id: Option<&str>,
    text_buf: &str,
    tool_status: &str,
    max_len: usize,
    last_edit: &mut Instant,
    edit_broken: &mut bool,
) {
    let Some(id) = msg_id else { return };
    let display = compose_display(text_buf, tool_status, max_len);
    if let Err(e) = sink.edit_text(chat_id, id, &display).await {
        tracing::warn!(error = %e, "delivery: edit failed");
        *edit_broken = true;
    } else {
        *last_edit = Instant::now();
    }
}

fn compose_display(text: &str, tool_status: &str, max_len: usize) -> String {
    let reserve = 80;
    let max = max_len.saturating_sub(reserve);
    let mut display = truncate_safe(text, max);
    if !tool_status.is_empty() {
        display.push_str(&format!("\n\n_{tool_status}_"));
    }
    display.push_str(" \u{2026}");
    display
}

fn truncate_safe(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let boundary = text
        .char_indices()
        .take_while(|(i, _)| *i <= max_len)
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0);
    text[..boundary].to_string()
}
