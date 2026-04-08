use tokio::sync::mpsc;

use super::transcript::assistant_blocks_from_content;
use super::transcript::extract_content_text;
use super::transcript::from_agent_messages;
use super::transcript::into_agent_messages;
use super::transcript::total_usage;
use crate::conf::ProviderKind;
use crate::error::Result;
use crate::protocol::model::run::ProtocolEvent;
use crate::protocol::model::run::UsageSummary;
use crate::protocol::model::transcript::TranscriptItem;

pub struct EngineOptions {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub system_prompt: String,
    pub limits: crate::agent::ExecutionLimits,
    pub skills_dirs: Vec<std::path::PathBuf>,
}

/// Handle to a running engine instance.
/// Provides access to final transcripts and abort capability.
pub struct EngineHandle {
    agent: Option<bend_engine::Agent>,
}

impl EngineHandle {
    /// Wait for the engine to finish, then extract final transcripts.
    /// Calls agent.finish().await first to ensure the agent loop is fully done.
    pub async fn take_transcripts(&mut self) -> Vec<TranscriptItem> {
        if let Some(agent) = self.agent.as_mut() {
            agent.finish().await;
            let messages = agent.messages().to_vec();
            return from_agent_messages(&messages);
        }
        Vec::new()
    }

    /// Abort the engine run.
    pub fn abort(&self) {
        if let Some(agent) = self.agent.as_ref() {
            agent.abort();
        }
    }
}

/// Start the engine and return a ProtocolEvent receiver + handle.
///
/// Internally builds a `bend_engine::Agent`, starts it with the given prompt,
/// and spawns a forwarder task that converts `AgentEvent` → `ProtocolEvent`.
pub async fn start_engine(
    options: &EngineOptions,
    prior_transcripts: &[TranscriptItem],
    prompt: String,
) -> Result<(mpsc::UnboundedReceiver<ProtocolEvent>, EngineHandle)> {
    let prior_messages = into_agent_messages(prior_transcripts);
    let mut agent = build_agent(options, prior_messages);
    let engine_rx = agent.prompt(prompt).await;

    let (tx, rx) = mpsc::unbounded_channel();

    // Forwarder task: AgentEvent → ProtocolEvent
    tokio::spawn(async move {
        forward_events(engine_rx, tx).await;
    });

    let handle = EngineHandle { agent: Some(agent) };
    Ok((rx, handle))
}

async fn forward_events(
    mut engine_rx: mpsc::UnboundedReceiver<bend_engine::AgentEvent>,
    tx: mpsc::UnboundedSender<ProtocolEvent>,
) {
    while let Some(event) = engine_rx.recv().await {
        let protocol_event = match &event {
            bend_engine::AgentEvent::AgentStart => Some(ProtocolEvent::AgentStart),
            bend_engine::AgentEvent::AgentEnd { messages } => {
                let transcripts = from_agent_messages(messages);
                let usage = total_usage(messages);
                let transcript_count = messages.len();
                Some(ProtocolEvent::AgentEnd {
                    transcripts,
                    usage,
                    transcript_count,
                })
            }
            bend_engine::AgentEvent::TurnStart => Some(ProtocolEvent::TurnStart),
            bend_engine::AgentEvent::TurnEnd { .. } => Some(ProtocolEvent::TurnEnd),
            bend_engine::AgentEvent::MessageStart { .. } => None,
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Text { delta },
                ..
            } => Some(ProtocolEvent::AssistantDelta {
                delta: Some(delta.clone()),
                thinking_delta: None,
            }),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::Thinking { delta },
                ..
            } => Some(ProtocolEvent::AssistantDelta {
                delta: None,
                thinking_delta: Some(delta.clone()),
            }),
            bend_engine::AgentEvent::MessageUpdate {
                delta: bend_engine::StreamDelta::ToolCallDelta { .. },
                ..
            } => None,
            bend_engine::AgentEvent::MessageEnd { message } => {
                if let bend_engine::AgentMessage::Llm(bend_engine::Message::Assistant {
                    content,
                    usage,
                    stop_reason,
                    error_message,
                    ..
                }) = message
                {
                    let blocks = assistant_blocks_from_content(content);
                    let usage_summary = UsageSummary {
                        input: usage.input,
                        output: usage.output,
                        cache_read: usage.cache_read,
                        cache_write: usage.cache_write,
                    };
                    Some(ProtocolEvent::AssistantCompleted {
                        content: blocks,
                        usage: Some(usage_summary),
                        stop_reason: stop_reason.to_string(),
                        error_message: error_message.clone(),
                    })
                } else {
                    None
                }
            }
            bend_engine::AgentEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
                preview_command,
            } => Some(ProtocolEvent::ToolStart {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
                preview_command: preview_command.clone(),
            }),
            bend_engine::AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                tool_name,
                partial_result,
            } => {
                let text = extract_content_text(&partial_result.content);
                Some(ProtocolEvent::ToolProgress {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    text,
                })
            }
            bend_engine::AgentEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
                result_tokens,
            } => {
                let content = extract_content_text(&result.content);
                Some(ProtocolEvent::ToolEnd {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    content,
                    is_error: *is_error,
                    details: result.details.clone(),
                    result_tokens: *result_tokens,
                })
            }
            bend_engine::AgentEvent::ProgressMessage {
                tool_call_id,
                tool_name,
                text,
            } => Some(ProtocolEvent::ToolProgress {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                text: text.clone(),
            }),
            bend_engine::AgentEvent::InputRejected { reason } => {
                Some(ProtocolEvent::InputRejected {
                    reason: reason.clone(),
                })
            }
            bend_engine::AgentEvent::LlmCallStart {
                turn,
                attempt,
                request,
            } => {
                let message_count = request.messages.len();
                let system_prompt_tokens =
                    bend_engine::context::estimate_tokens(&request.system_prompt);
                let messages: Vec<serde_json::Value> = request
                    .messages
                    .iter()
                    .map(|m| serde_json::to_value(m).unwrap_or_default())
                    .collect();
                let tools: Vec<serde_json::Value> = request
                    .tools
                    .iter()
                    .map(|t| serde_json::to_value(t).unwrap_or_default())
                    .collect();
                Some(ProtocolEvent::LlmCallStart {
                    turn: *turn,
                    attempt: *attempt,
                    model: request.model.clone(),
                    system_prompt: request.system_prompt.clone(),
                    messages,
                    tools,
                    message_count,
                    system_prompt_tokens,
                })
            }
            bend_engine::AgentEvent::LlmCallEnd {
                turn,
                attempt,
                usage,
                error,
            } => Some(ProtocolEvent::LlmCallEnd {
                turn: *turn,
                attempt: *attempt,
                usage: UsageSummary {
                    input: usage.input,
                    output: usage.output,
                    cache_read: usage.cache_read,
                    cache_write: usage.cache_write,
                },
                cache_read: usage.cache_read,
                cache_write: usage.cache_write,
                error: error.clone(),
            }),
            bend_engine::AgentEvent::ContextCompactionStart {
                message_count,
                estimated_tokens,
                budget_tokens,
                system_prompt_tokens,
                context_window,
            } => Some(ProtocolEvent::ContextCompactionStart {
                message_count: *message_count,
                estimated_tokens: *estimated_tokens,
                budget_tokens: *budget_tokens,
                system_prompt_tokens: *system_prompt_tokens,
                context_window: *context_window,
            }),
            bend_engine::AgentEvent::ContextCompactionEnd { stats, messages } => {
                let compacted_transcripts = from_agent_messages(messages);
                let before_tool_details: Vec<(String, usize)> = stats
                    .before_tool_details
                    .iter()
                    .map(|d| (d.tool_name.clone(), d.tokens))
                    .collect();
                let after_tool_details: Vec<(String, usize)> = stats
                    .after_tool_details
                    .iter()
                    .map(|d| (d.tool_name.clone(), d.tokens))
                    .collect();
                Some(ProtocolEvent::ContextCompactionEnd {
                    level: stats.level,
                    before_message_count: stats.before_message_count,
                    after_message_count: stats.after_message_count,
                    before_estimated_tokens: stats.before_estimated_tokens,
                    after_estimated_tokens: stats.after_estimated_tokens,
                    tool_outputs_truncated: stats.tool_outputs_truncated,
                    turns_summarized: stats.turns_summarized,
                    messages_dropped: stats.messages_dropped,
                    before_tool_details,
                    after_tool_details,
                    compacted_transcripts,
                })
            }
        };

        if let Some(pe) = protocol_event {
            if tx.send(pe).is_err() {
                break;
            }
        }
    }
}

fn build_agent(
    options: &EngineOptions,
    prior_messages: Vec<bend_engine::AgentMessage>,
) -> bend_engine::Agent {
    use bend_engine::provider::AnthropicProvider;
    use bend_engine::provider::ModelConfig;
    use bend_engine::provider::OpenAiCompatProvider;

    let mut model_config = match options.provider {
        ProviderKind::Anthropic => ModelConfig::anthropic(&options.model, &options.model),
        ProviderKind::OpenAi => ModelConfig::openai(&options.model, &options.model),
    };
    if let Some(base_url) = &options.base_url {
        model_config.base_url = base_url.clone();
    }

    let provider_agent = match options.provider {
        ProviderKind::Anthropic => bend_engine::Agent::new(AnthropicProvider),
        ProviderKind::OpenAi => bend_engine::Agent::new(OpenAiCompatProvider),
    };

    let limits = bend_engine::context::ExecutionLimits {
        max_turns: options.limits.max_turns as usize,
        max_total_tokens: options.limits.max_total_tokens as usize,
        max_duration: std::time::Duration::from_secs(options.limits.max_duration_secs),
    };

    let skills = if options.skills_dirs.is_empty() {
        bend_engine::SkillSet::empty()
    } else {
        match bend_engine::SkillSet::load(&options.skills_dirs) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to load skills: {e}");
                bend_engine::SkillSet::empty()
            }
        }
    };

    provider_agent
        .with_model(&options.model)
        .with_api_key(&options.api_key)
        .with_model_config(model_config)
        .with_system_prompt(&options.system_prompt)
        .with_user_agent(format!(
            "bendclaw/{} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
        ))
        .with_skills(skills)
        .with_tools(bend_engine::tools::default_tools())
        .with_messages(prior_messages)
        .with_execution_limits(limits)
}
