use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio_stream::StreamExt;

use crate::base::truncate_bytes_on_char_boundary;
use crate::base::Result;
use crate::kernel::channel::diagnostics;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;

const STREAM_CHUNK_TIMEOUT: Duration = Duration::from_secs(300);
const BROKEN_RETRY_INTERVAL: u32 = 5;

pub struct StreamDeliveryConfig {
    /// Minimum interval between edits (ms).
    pub throttle_ms: u64,
    /// Send the first draft after accumulating this many chars.
    pub min_initial_chars: usize,
    /// Channel's max message length.
    pub max_message_len: usize,
    /// Whether to show tool execution progress.
    pub show_tool_progress: bool,
}

pub struct StreamDelivery {
    config: StreamDeliveryConfig,
    outbound: Arc<dyn ChannelOutbound>,
    channel_config: serde_json::Value,
    chat_id: String,
}

impl StreamDelivery {
    pub fn new(
        config: StreamDeliveryConfig,
        outbound: Arc<dyn ChannelOutbound>,
        channel_config: serde_json::Value,
        chat_id: String,
    ) -> Self {
        Self {
            config,
            outbound,
            channel_config,
            chat_id,
        }
    }

    /// Consume the event stream, deliver incrementally to channel.
    /// Returns the final output text.
    pub async fn deliver<S>(&self, stream: &mut S) -> Result<String>
    where S: tokio_stream::Stream<Item = Event> + Unpin {
        let mut text_buf = String::new();
        let mut draft_msg_id: Option<String> = None;
        let mut last_edit = Instant::now();
        let mut tool_status = String::new();
        let mut draft_broken = false;
        let mut broken_skip_count: u32 = 0;

        loop {
            let ev = match tokio::time::timeout(STREAM_CHUNK_TIMEOUT, stream.next()).await {
                Ok(Some(ev)) => ev,
                Ok(None) => break,
                Err(_) => {
                    diagnostics::log_channel_stream_timeout(STREAM_CHUNK_TIMEOUT.as_secs());
                    break;
                }
            };

            match &ev {
                Event::StreamDelta(Delta::Text { content }) => {
                    text_buf.push_str(content);

                    if draft_msg_id.is_none() && text_buf.len() >= self.config.min_initial_chars {
                        let display = self.compose_display(&text_buf, &tool_status);
                        match self
                            .outbound
                            .send_draft(&self.channel_config, &self.chat_id, &display)
                            .await
                        {
                            Ok(id) => {
                                draft_msg_id = Some(id);
                                draft_broken = false;
                                broken_skip_count = 0;
                                last_edit = Instant::now();
                            }
                            Err(e) => {
                                diagnostics::log_channel_send_draft_failed(Some(&e));
                            }
                        }
                    } else if draft_msg_id.is_some()
                        && self.should_attempt_update(draft_broken, &mut broken_skip_count)
                        && last_edit.elapsed().as_millis() as u64 >= self.config.throttle_ms
                    {
                        self.do_update(
                            &draft_msg_id,
                            &text_buf,
                            &tool_status,
                            &mut last_edit,
                            &mut draft_broken,
                            &mut broken_skip_count,
                        )
                        .await;
                    }
                }
                Event::ToolStart { name, .. } if self.config.show_tool_progress => {
                    tool_status = format!("\u{1F527} {name}...");
                    if self.should_attempt_update(draft_broken, &mut broken_skip_count) {
                        self.do_update(
                            &draft_msg_id,
                            &text_buf,
                            &tool_status,
                            &mut last_edit,
                            &mut draft_broken,
                            &mut broken_skip_count,
                        )
                        .await;
                    }
                }
                Event::ToolEnd { name, success, .. } if self.config.show_tool_progress => {
                    let icon = if *success { "\u{2705}" } else { "\u{274C}" };
                    tool_status = format!("{icon} {name}");
                    if self.should_attempt_update(draft_broken, &mut broken_skip_count) {
                        self.do_update(
                            &draft_msg_id,
                            &text_buf,
                            &tool_status,
                            &mut last_edit,
                            &mut draft_broken,
                            &mut broken_skip_count,
                        )
                        .await;
                    }
                }
                Event::Progress { message, .. } if self.config.show_tool_progress => {
                    tool_status = format!("\u{23F3} {message}");
                    if self.should_attempt_update(draft_broken, &mut broken_skip_count) {
                        self.do_update(
                            &draft_msg_id,
                            &text_buf,
                            &tool_status,
                            &mut last_edit,
                            &mut draft_broken,
                            &mut broken_skip_count,
                        )
                        .await;
                    }
                }
                Event::ReasonStart => {
                    tool_status.clear();
                }
                _ => {}
            }
        }

        let final_text = self.truncate(&text_buf);
        if let Some(ref msg_id) = draft_msg_id {
            if !draft_broken {
                if let Err(e) = self
                    .outbound
                    .finalize_draft(&self.channel_config, &self.chat_id, msg_id, &final_text)
                    .await
                {
                    diagnostics::log_channel_finalize_draft_failed(Some(&e));
                }
            }
        }

        Ok(text_buf)
    }

    fn should_attempt_update(&self, draft_broken: bool, skip_count: &mut u32) -> bool {
        if !draft_broken {
            return true;
        }
        *skip_count += 1;
        if *skip_count >= BROKEN_RETRY_INTERVAL {
            *skip_count = 0;
            return true;
        }
        false
    }

    async fn do_update(
        &self,
        draft_msg_id: &Option<String>,
        text_buf: &str,
        tool_status: &str,
        last_edit: &mut Instant,
        draft_broken: &mut bool,
        broken_skip_count: &mut u32,
    ) {
        match self
            .try_update(draft_msg_id, text_buf, tool_status, last_edit)
            .await
        {
            Ok(()) => {
                *draft_broken = false;
                *broken_skip_count = 0;
            }
            Err(()) => {
                *draft_broken = true;
            }
        }
    }

    async fn try_update(
        &self,
        draft_msg_id: &Option<String>,
        text_buf: &str,
        tool_status: &str,
        last_edit: &mut Instant,
    ) -> std::result::Result<(), ()> {
        let Some(ref msg_id) = draft_msg_id else {
            return Ok(());
        };
        let display = self.compose_display(text_buf, tool_status);
        if let Err(e) = self
            .outbound
            .update_draft(&self.channel_config, &self.chat_id, msg_id, &display)
            .await
        {
            diagnostics::log_channel_update_draft_failed(&e);
            *last_edit = Instant::now();
            return Err(());
        }
        *last_edit = Instant::now();
        Ok(())
    }

    fn compose_display(&self, text: &str, tool_status: &str) -> String {
        let reserve = 100;
        let max = self.config.max_message_len.saturating_sub(reserve);
        let mut display = if text.len() > max {
            truncate_bytes_on_char_boundary(text, max)
        } else {
            text.to_string()
        };
        if !tool_status.is_empty() {
            display.push_str(&format!("\n\n_{tool_status}_"));
        }
        display.push_str(" \u{2026}");
        display
    }

    fn truncate(&self, text: &str) -> String {
        if text.len() > self.config.max_message_len {
            truncate_bytes_on_char_boundary(text, self.config.max_message_len)
        } else {
            text.to_string()
        }
    }
}
