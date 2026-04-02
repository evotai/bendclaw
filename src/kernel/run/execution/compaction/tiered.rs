//! Tiered compaction strategy: L1 (truncate tool outputs) → L2 (drop old
//! tool results) → L3 (LLM summarization).

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::diagnostics;
use super::strategy::CompactionConfig;
use super::strategy::CompactionOutcome;
use super::strategy::CompactionStrategy;
use super::transcript;
use crate::kernel::run::prompt_projection;
use crate::kernel::Message;
use crate::llm::message::ChatMessage;
use crate::llm::provider::LLMProvider;
use crate::llm::usage::TokenUsage;

/// Maximum characters per chunk for staged summarization (~10K tokens).
const CHUNK_SIZE: usize = 40_000;

pub struct TieredCompactionStrategy {
    llm: Arc<dyn LLMProvider>,
    model: Arc<str>,
    cancel: CancellationToken,
}

impl TieredCompactionStrategy {
    pub fn new(llm: Arc<dyn LLMProvider>, model: Arc<str>, cancel: CancellationToken) -> Self {
        Self { llm, model, cancel }
    }

    // ── Level 1: truncate long tool outputs ──

    fn truncate_tool_outputs(messages: &[Message], max_lines: usize) -> Option<Vec<Message>> {
        let half = max_lines / 2;
        let mut changed = false;
        let compacted: Vec<Message> = messages
            .iter()
            .map(|m| match m {
                Message::ToolResult {
                    tool_call_id,
                    name,
                    output,
                    success,
                    origin_run_id: _,
                    operation,
                } => {
                    let lines: Vec<&str> = output.lines().collect();
                    if lines.len() > max_lines {
                        changed = true;
                        let head = &lines[..half];
                        let tail = &lines[lines.len() - half..];
                        let truncated = format!(
                            "{}\n\n... [{} lines truncated] ...\n\n{}",
                            head.join("\n"),
                            lines.len() - max_lines,
                            tail.join("\n"),
                        );
                        Message::tool_result_with_operation(
                            tool_call_id,
                            name,
                            &truncated,
                            *success,
                            operation.clone(),
                        )
                    } else {
                        m.clone()
                    }
                }
                _ => m.clone(),
            })
            .collect();

        if changed {
            Some(compacted)
        } else {
            None
        }
    }

    // ── Level 2: drop old ToolResult messages ──

    fn drop_old_tool_results(
        messages: &[Message],
        config: &CompactionConfig,
    ) -> Option<Vec<Message>> {
        if messages.len() <= config.keep_first + config.keep_recent {
            return None;
        }

        let middle_start = config.keep_first;
        let middle_end = messages.len().saturating_sub(config.keep_recent);
        if middle_start >= middle_end {
            return None;
        }

        let mut changed = false;
        let mut compacted = Vec::with_capacity(messages.len());

        // Keep first N
        compacted.extend_from_slice(&messages[..middle_start]);

        // Middle: drop ToolResult, keep everything else
        for msg in &messages[middle_start..middle_end] {
            match msg {
                Message::ToolResult { .. } => {
                    changed = true;
                }
                _ => compacted.push(msg.clone()),
            }
        }

        // Keep recent N
        compacted.extend_from_slice(&messages[middle_end..]);

        if changed {
            Some(compacted)
        } else {
            None
        }
    }

    // ── Level 3: LLM summarization ──

    async fn summarize(&self, dropped: &[&Message]) -> (Option<String>, TokenUsage) {
        let text = transcript::build_transcript(dropped);
        if text.is_empty() {
            return (None, TokenUsage::default());
        }

        let chunks = transcript::split_chunks(&text, CHUNK_SIZE);

        if chunks.len() <= 1 {
            return self.summarize_single(&text).await;
        }

        let mut total_usage = TokenUsage::default();
        let mut partial_summaries = Vec::new();
        for chunk in &chunks {
            let (summary, usage) = self.summarize_single(chunk).await;
            total_usage += &usage;
            if let Some(t) = summary {
                partial_summaries.push(t);
            }
        }

        let merged = partial_summaries.join("\n\n");
        if merged.is_empty() {
            return (None, total_usage);
        }

        let (summary, merge_usage) = self.summarize_single(&merged).await;
        total_usage += &merge_usage;
        (summary, total_usage)
    }

    async fn summarize_single(&self, text: &str) -> (Option<String>, TokenUsage) {
        let prompt = format!(
            "Summarize the following conversation excerpt in 2-4 concise paragraphs. \
             Focus on: key decisions, tool calls, important facts, unresolved questions. \
             No greetings or filler.\n\n{text}"
        );

        let messages = vec![ChatMessage::user(prompt)];

        tokio::select! {
            result = self.llm.chat(&self.model, &messages, &[], 0.0) => {
                match result {
                    Ok(resp) => {
                        let usage = resp.usage.unwrap_or_default();
                        let content = resp.content.filter(|s| !s.is_empty());
                        (content, usage)
                    }
                    Err(e) => {
                        diagnostics::log_compaction_summarize_failed(&e);
                        (None, TokenUsage::default())
                    }
                }
            }
            _ = self.cancel.cancelled() => {
                (None, TokenUsage::default())
            }
        }
    }
}

#[async_trait]
impl CompactionStrategy for TieredCompactionStrategy {
    async fn compact(
        &self,
        messages: Vec<Message>,
        config: &CompactionConfig,
        current_run_id: &str,
    ) -> Option<CompactionOutcome> {
        let budget = config.max_context_tokens;

        // Try L1: truncate tool outputs
        if let Some(l1) = Self::truncate_tool_outputs(&messages, config.tool_output_max_lines) {
            if prompt_projection::count_prompt_tokens(&l1) <= budget {
                return Some(CompactionOutcome {
                    messages: l1,
                    token_usage: TokenUsage::default(),
                    description: "L1: truncated tool outputs".into(),
                    checkpoint: None,
                });
            }
            // L1 wasn't enough, try L2 on L1 result
            if let Some(l2) = Self::drop_old_tool_results(&l1, config) {
                if prompt_projection::count_prompt_tokens(&l2) <= budget {
                    return Some(CompactionOutcome {
                        messages: l2,
                        token_usage: TokenUsage::default(),
                        description: "L1+L2: truncated outputs + dropped old results".into(),
                        checkpoint: None,
                    });
                }
                // L1+L2 not enough, fall through to L3 on L2 result
                return self.run_l3(l2, config, current_run_id).await;
            }
            // L2 had nothing to drop, go straight to L3 on L1 result
            return self.run_l3(l1, config, current_run_id).await;
        }

        // Try L2 directly
        if let Some(l2) = Self::drop_old_tool_results(&messages, config) {
            if prompt_projection::count_prompt_tokens(&l2) <= budget {
                return Some(CompactionOutcome {
                    messages: l2,
                    token_usage: TokenUsage::default(),
                    description: "L2: dropped old tool results".into(),
                    checkpoint: None,
                });
            }
            return self.run_l3(l2, config, current_run_id).await;
        }

        // Go straight to L3
        self.run_l3(messages, config, current_run_id).await
    }
}

impl TieredCompactionStrategy {
    async fn run_l3(
        &self,
        messages: Vec<Message>,
        config: &CompactionConfig,
        current_run_id: &str,
    ) -> Option<CompactionOutcome> {
        use super::rules::plan_compaction_split;
        use crate::kernel::run::checkpoint::CompactionCheckpoint;
        use crate::kernel::run::prompt_projection::prompt_token_vec;

        let msg_tokens = prompt_token_vec(&messages);
        let plan = plan_compaction_split(&messages, &msg_tokens, config.max_context_tokens)?;
        let split = plan.split_index;

        // Count how many non-system messages appear before split in the
        // original array. This is the correct index into the non_system vec
        // after partition, because System/CompactionSummary are pulled out.
        let non_system_split = messages[..split]
            .iter()
            .filter(|m| {
                !matches!(
                    m,
                    Message::System { .. } | Message::CompactionSummary { .. }
                )
            })
            .count();

        if non_system_split == 0 {
            return None;
        }

        let (system, non_system): (Vec<Message>, Vec<Message>) =
            messages.into_iter().partition(|m| {
                matches!(
                    m,
                    Message::System { .. } | Message::CompactionSummary { .. }
                )
            });

        let dropped: Vec<&Message> = non_system.iter().take(non_system_split).collect();
        let (summary, token_usage) = self.summarize(&dropped).await;

        // Build checkpoint if dropped messages are all from prior runs
        let checkpoint = summary.as_ref().and_then(|summary_text| {
            let mut last_run_id = None;
            for msg in &dropped {
                match msg.origin_run_id() {
                    Some(rid) if rid == current_run_id => return None,
                    Some(rid) => last_run_id = Some(rid),
                    None => return None,
                }
            }
            last_run_id.map(|through_run_id| CompactionCheckpoint {
                summary_text: summary_text.clone(),
                through_run_id: through_run_id.to_string(),
            })
        });

        let mut compacted = system;
        if let Some(text) = summary {
            compacted.push(Message::compaction(&text));
        }
        compacted.extend(non_system.into_iter().skip(non_system_split));

        Some(CompactionOutcome {
            messages: compacted,
            token_usage,
            description: "L3: LLM summarization".into(),
            checkpoint,
        })
    }
}
