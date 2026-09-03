//! Tool execution: sequential, batch, and single-tool dispatch.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::config::GetMessagesFn;
use crate::context;
use crate::context::now_ms;
use crate::provider::ToolDefinition;
use crate::spill::FsSpill;
use crate::tools::guard::PathGuard;
use crate::types::*;

/// Resolve per-model tool names and descriptions into provider definitions.
pub(super) fn build_tool_definitions(context: &AgentContext, model: &str) -> Vec<ToolDefinition> {
    context
        .tools
        .iter()
        .map(|tool| ToolDefinition {
            name: tool.resolve_name(model),
            description: crate::tools::resolve_tool_refs(tool.description(), &context.tools, model),
            parameters: tool.parameters_schema(),
        })
        .collect()
}

pub(super) struct ToolExecutionResult {
    pub tool_results: Vec<Message>,
    pub steering_messages: Option<Vec<AgentMessage>>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_tool_calls(
    tools: &[Box<dyn AgentTool>],
    tool_calls: &[(String, String, serde_json::Value)],
    model: &str,
    provider: &str,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
    get_steering: Option<&GetMessagesFn>,
    strategy: &ToolExecutionStrategy,
    cwd: &std::path::Path,
    path_guard: &Arc<PathGuard>,
    spill: &Option<Arc<FsSpill>>,
    idle_clock: Option<&crate::context::IdleClock>,
    supports_image: bool,
) -> ToolExecutionResult {
    let fanout_batch_size = match strategy {
        ToolExecutionStrategy::Sequential => 1,
        ToolExecutionStrategy::Parallel => usize::MAX,
        ToolExecutionStrategy::Batched { size } => *size,
    };
    match strategy {
        ToolExecutionStrategy::Sequential => {
            execute_sequential(
                tools,
                tool_calls,
                model,
                provider,
                tx,
                cancel,
                get_steering,
                cwd,
                path_guard,
                spill,
                idle_clock,
                supports_image,
                fanout_batch_size,
            )
            .await
        }
        ToolExecutionStrategy::Parallel => {
            execute_batch(
                tools,
                tool_calls,
                model,
                provider,
                tx,
                cancel,
                get_steering,
                cwd,
                path_guard,
                spill,
                idle_clock,
                supports_image,
                fanout_batch_size,
            )
            .await
        }
        ToolExecutionStrategy::Batched { size } => {
            let mut results: Vec<Message> = Vec::new();
            let mut steering_messages: Option<Vec<AgentMessage>> = None;

            for (batch_idx, batch) in tool_calls.chunks(*size).enumerate() {
                let batch_result = execute_batch(
                    tools,
                    batch,
                    model,
                    provider,
                    tx,
                    cancel,
                    None,
                    cwd,
                    path_guard,
                    spill,
                    idle_clock,
                    supports_image,
                    fanout_batch_size,
                )
                .await;
                results.extend(batch_result.tool_results);

                // Check steering between batches
                if let Some(get_steering_fn) = get_steering {
                    let steering = get_steering_fn();
                    if !steering.is_empty() {
                        steering_messages = Some(steering);
                        // Skip remaining batches
                        let executed = (batch_idx + 1) * *size;
                        if executed < tool_calls.len() {
                            for (skip_id, skip_name, _) in &tool_calls[executed..] {
                                results.push(skip_tool_call(skip_id, skip_name, tx));
                            }
                        }
                        break;
                    }
                }
            }

            ToolExecutionResult {
                tool_results: results,
                steering_messages,
            }
        }
    }
}

/// Execute tool calls one at a time, checking steering between each.
#[allow(clippy::too_many_arguments)]
async fn execute_sequential(
    tools: &[Box<dyn AgentTool>],
    tool_calls: &[(String, String, serde_json::Value)],
    model: &str,
    provider: &str,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
    get_steering: Option<&GetMessagesFn>,
    cwd: &std::path::Path,
    path_guard: &Arc<PathGuard>,
    spill: &Option<Arc<FsSpill>>,
    idle_clock: Option<&crate::context::IdleClock>,
    supports_image: bool,
    fanout_batch_size: usize,
) -> ToolExecutionResult {
    let mut results: Vec<Message> = Vec::new();
    let mut steering_messages: Option<Vec<AgentMessage>> = None;

    for (index, (id, name, args)) in tool_calls.iter().enumerate() {
        let (msg, _is_error) = execute_single_tool(
            tools,
            id,
            name,
            args,
            model,
            provider,
            tx,
            cancel,
            cwd,
            path_guard,
            spill,
            idle_clock,
            supports_image,
            fanout_batch_size,
        )
        .await;
        results.push(msg);

        // Check for steering — skip remaining tools if user interrupted
        if let Some(get_steering_fn) = get_steering {
            let steering = get_steering_fn();
            if !steering.is_empty() {
                steering_messages = Some(steering);
                for (skip_id, skip_name, _) in &tool_calls[index + 1..] {
                    results.push(skip_tool_call(skip_id, skip_name, tx));
                }
                break;
            }
        }
    }

    ToolExecutionResult {
        tool_results: results,
        steering_messages,
    }
}

/// Execute a batch of tool calls concurrently using futures::join_all.
#[allow(clippy::too_many_arguments)]
async fn execute_batch(
    tools: &[Box<dyn AgentTool>],
    tool_calls: &[(String, String, serde_json::Value)],
    model: &str,
    provider: &str,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
    get_steering: Option<&GetMessagesFn>,
    cwd: &std::path::Path,
    path_guard: &Arc<PathGuard>,
    spill: &Option<Arc<FsSpill>>,
    idle_clock: Option<&crate::context::IdleClock>,
    supports_image: bool,
    fanout_batch_size: usize,
) -> ToolExecutionResult {
    use futures::future::join_all;

    let futures: Vec<_> = tool_calls
        .iter()
        .map(|(id, name, args)| {
            execute_single_tool(
                tools,
                id,
                name,
                args,
                model,
                provider,
                tx,
                cancel,
                cwd,
                path_guard,
                spill,
                idle_clock,
                supports_image,
                fanout_batch_size,
            )
        })
        .collect();

    let batch_results = join_all(futures).await;

    let results: Vec<Message> = batch_results.into_iter().map(|(msg, _)| msg).collect();

    // Check steering after batch completes
    let steering_messages = if let Some(get_steering_fn) = get_steering {
        let steering = get_steering_fn();
        if steering.is_empty() {
            None
        } else {
            Some(steering)
        }
    } else {
        None
    };

    ToolExecutionResult {
        tool_results: results,
        steering_messages,
    }
}

/// Execute one upstream tool call and emit one correlated event lifecycle.
/// Compatibility fan-out stays internal so providers still receive exactly one
/// result for the original tool-call id.
#[allow(clippy::too_many_arguments)]
async fn execute_single_tool(
    tools: &[Box<dyn AgentTool>],
    id: &str,
    name: &str,
    args: &serde_json::Value,
    model: &str,
    provider: &str,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: &tokio_util::sync::CancellationToken,
    cwd: &std::path::Path,
    path_guard: &Arc<PathGuard>,
    spill: &Option<Arc<FsSpill>>,
    idle_clock: Option<&crate::context::IdleClock>,
    supports_image: bool,
    fanout_batch_size: usize,
) -> (Message, bool) {
    let tool_index = tools
        .iter()
        .position(|candidate| candidate.matches_call_name(name));
    let fanout = tool_index
        .and_then(|index| super::tool_fanout::arguments(&tools[index].parameters_schema(), args));
    let input_shape = json_type_name(args);
    let fanout_count = fanout.as_ref().map_or(0, Vec::len);
    let shape_mismatch = tool_index.is_some_and(|index| {
        tools[index].parameters_schema().get("properties").is_some() && !args.is_object()
    });

    let preview_command = tool_index.and_then(|index| tools[index].preview_command(args));
    tx.send(AgentEvent::ToolExecutionStart {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        args: args.clone(),
        preview_command,
        is_fanout: fanout_count > 0,
        invocation_count: fanout_count.max(1),
        parallel: fanout_count > 1 && fanout_batch_size > 1,
    })
    .ok();

    if fanout_count > 0 {
        tracing::info!(
            tool_name = name,
            tool_call_id = id,
            input_shape,
            fanout_count,
            "fanning out batched tool arguments"
        );
        tx.send(AgentEvent::ToolInputDiagnostic {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            input_shape: input_shape.to_string(),
            fanout_count,
        })
        .ok();
    } else if shape_mismatch {
        tracing::warn!(
            tool_name = name,
            tool_call_id = id,
            input_shape,
            "tool input shape mismatch"
        );
        tx.send(AgentEvent::ToolInputDiagnostic {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            input_shape: input_shape.to_string(),
            fanout_count: 0,
        })
        .ok();
    }

    let tool_start = std::time::Instant::now();
    let base_context = InvocationContext {
        id: id.to_string(),
        name: name.to_string(),
        tx: tx.clone(),
        cancel: cancel.child_token(),
        cwd: cwd.to_path_buf(),
        path_guard: path_guard.clone(),
        spill: spill.clone(),
        supports_image,
    };
    // One upstream call contributes one idle interval even when compatibility
    // fan-out runs several child invocations concurrently.
    let _idle = idle_clock.map(|clock| clock.pause());

    let (result, is_error) = match (tool_index, fanout) {
        (Some(preferred_index), Some(items)) => {
            let results = execute_fanout(
                tools,
                preferred_index,
                items,
                &base_context,
                fanout_batch_size,
            )
            .await;
            super::tool_fanout::aggregate(results)
        }
        (Some(index), _) => execute_invocation(tools[index].as_ref(), args, &base_context).await,
        (None, _) => (
            ToolResult {
                content: vec![Content::Text {
                    text: format!("Tool {name} not found"),
                }],
                details: serde_json::Value::Null,
                retention: Retention::Normal,
            },
            true,
        ),
    };

    // System-level tool result size management. Fan-out children have already
    // been individually capped; this applies the aggregate cap/spill policy.
    let (result, spill_event) = process_result(spill, id, name, result, is_error).await;
    if let Some(event) = spill_event {
        tx.send(AgentEvent::ProgressMessage {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            text: event.to_progress_text(),
        })
        .ok();
    }

    let result_tokens = context::content_tokens(&result.content);
    let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
    tx.send(AgentEvent::ToolExecutionEnd {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        result: result.clone(),
        is_error,
        result_tokens,
        duration_ms: tool_duration_ms,
    })
    .ok();

    let tool_result_msg = Message::ToolResult {
        tool_call_id: id.to_string(),
        tool_name: name.to_string(),
        content: result.content,
        is_error,
        timestamp: now_ms(),
        retention: result.retention,
    };
    tx.send(AgentEvent::MessageStart {
        message: tool_result_msg.clone().into(),
    })
    .ok();
    tx.send(AgentEvent::MessageEnd {
        message: tool_result_msg.clone().into(),
    })
    .ok();

    (tool_result_msg, is_error)
}

async fn execute_fanout(
    tools: &[Box<dyn AgentTool>],
    preferred_index: usize,
    items: Vec<serde_json::Value>,
    base_context: &InvocationContext,
    batch_size: usize,
) -> Vec<(String, Option<String>, ToolResult, bool)> {
    use futures::future::join_all;

    let batch_size = batch_size.max(1);
    let mut results = Vec::with_capacity(items.len());
    for (batch_index, batch) in items.chunks(batch_size).enumerate() {
        let futures = batch.iter().cloned().enumerate().map(|(index, item)| {
            let absolute_index = batch_index.saturating_mul(batch_size) + index;
            let selected_index = select_tool_index(tools, preferred_index, &item);
            let selected_tool = tools[selected_index].as_ref();
            let selected_name = selected_tool.name().to_string();
            let preview_command = selected_tool.preview_command(&item);
            let context = base_context.with_name(selected_name.clone());
            async move {
                let (result, is_error) = execute_invocation(selected_tool, &item, &context).await;
                let child_id = format!("{}:{}", context.id, absolute_index + 1);
                let (result, spill_event) =
                    process_result(&context.spill, &child_id, &context.name, result, is_error)
                        .await;
                if let Some(event) = spill_event {
                    context
                        .tx
                        .send(AgentEvent::ProgressMessage {
                            tool_call_id: context.id.clone(),
                            tool_name: context.name.clone(),
                            text: event.to_progress_text(),
                        })
                        .ok();
                }
                (selected_name, preview_command, result, is_error)
            }
        });
        results.extend(join_all(futures).await);
    }
    results
}

#[derive(Clone)]
struct InvocationContext {
    id: String,
    name: String,
    tx: mpsc::UnboundedSender<AgentEvent>,
    cancel: tokio_util::sync::CancellationToken,
    cwd: std::path::PathBuf,
    path_guard: Arc<PathGuard>,
    spill: Option<Arc<FsSpill>>,
    supports_image: bool,
}

impl InvocationContext {
    fn with_name(&self, name: String) -> Self {
        let mut context = self.clone();
        context.name = name;
        context
    }
}

fn select_tool_index(
    tools: &[Box<dyn AgentTool>],
    preferred_index: usize,
    args: &serde_json::Value,
) -> usize {
    if prepare_and_validate(tools[preferred_index].as_ref(), args).is_ok() {
        return preferred_index;
    }

    let mut matches = tools
        .iter()
        .enumerate()
        .filter(|(index, tool)| {
            *index != preferred_index
                && has_required_parameters(&tool.parameters_schema())
                && prepare_and_validate(tool.as_ref(), args).is_ok()
        })
        .map(|(index, _)| index);
    let first = matches.next();
    match (first, matches.next()) {
        (Some(index), None) => index,
        _ => preferred_index,
    }
}

fn has_required_parameters(schema: &serde_json::Value) -> bool {
    schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|required| !required.is_empty())
}

async fn execute_invocation(
    tool: &dyn AgentTool,
    args: &serde_json::Value,
    invocation: &InvocationContext,
) -> (ToolResult, bool) {
    let on_update: Option<ToolUpdateFn> = {
        let tx = invocation.tx.clone();
        let id = invocation.id.to_string();
        let name = invocation.name.to_string();
        Some(Arc::new(move |partial: ToolResult| {
            tx.send(AgentEvent::ToolExecutionUpdate {
                tool_call_id: id.clone(),
                tool_name: name.clone(),
                partial_result: partial,
            })
            .ok();
        }))
    };
    let on_progress: Option<ProgressFn> = {
        let tx = invocation.tx.clone();
        let id = invocation.id.to_string();
        let name = invocation.name.to_string();
        Some(Arc::new(move |text: String| {
            tx.send(AgentEvent::ProgressMessage {
                tool_call_id: id.clone(),
                tool_name: name.clone(),
                text,
            })
            .ok();
        }))
    };
    let ctx = ToolContext {
        tool_call_id: invocation.id.clone(),
        tool_name: invocation.name.clone(),
        cancel: invocation.cancel.child_token(),
        on_update,
        on_progress,
        cwd: invocation.cwd.clone(),
        path_guard: invocation.path_guard.clone(),
        spill: invocation.spill.clone(),
        supports_image: invocation.supports_image,
    };

    let coerced_args = match prepare_and_validate(tool, args) {
        Ok(value) => value,
        Err(error) => {
            return (
                ToolResult {
                    content: vec![Content::Text {
                        text: crate::tools::validation::truncate_error(&error),
                    }],
                    details: serde_json::Value::Null,
                    retention: Retention::Normal,
                },
                true,
            );
        }
    };

    match tool.execute(coerced_args, ctx).await {
        Ok(result) => (result, false),
        Err(error) => (
            ToolResult {
                content: vec![Content::Text {
                    text: error.to_string(),
                }],
                details: serde_json::Value::Null,
                retention: Retention::Normal,
            },
            true,
        ),
    }
}

fn prepare_and_validate(
    tool: &dyn AgentTool,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut normalized = match tool.parameter_aliases() {
        Some(aliases) => crate::tools::validation::normalize_aliases(args, aliases),
        None => args.clone(),
    };
    normalized = tool.prepare_arguments(&normalized);
    crate::tools::validation::validate_and_coerce_with_received(
        tool.name(),
        &tool.parameters_schema(),
        &normalized,
        args,
    )
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

pub(super) fn fail_truncated_tool_calls(
    tool_calls: &[(String, String, serde_json::Value)],
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<Message> {
    tool_calls
        .iter()
        .map(|(tool_call_id, tool_name, args)| {
            let result = ToolResult {
                content: vec![Content::Text {
                    text: format!(
                        "Tool call \"{tool_name}\" was not executed: the response hit the output token limit, so its arguments may be truncated. Re-issue the tool call with complete arguments."
                    ),
                }],
                details: serde_json::Value::Null,
                retention: Retention::Normal,
            };

            tx.send(AgentEvent::ToolExecutionStart {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                args: args.clone(),
                preview_command: None,
                is_fanout: false,
                invocation_count: 1,
                parallel: false,
            })
            .ok();

            let result_tokens = context::content_tokens(&result.content);
            tx.send(AgentEvent::ToolExecutionEnd {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                result: result.clone(),
                is_error: true,
                result_tokens,
                duration_ms: 0,
            })
            .ok();

            let message = Message::ToolResult {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.clone(),
                content: result.content,
                is_error: true,
                timestamp: now_ms(),
                retention: Retention::Normal,
            };
            tx.send(AgentEvent::MessageStart {
                message: message.clone().into(),
            })
            .ok();
            tx.send(AgentEvent::MessageEnd {
                message: message.clone().into(),
            })
            .ok();
            message
        })
        .collect()
}

pub(super) fn skip_tool_call(
    tool_call_id: &str,
    tool_name: &str,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Message {
    let result = ToolResult {
        content: vec![Content::Text {
            text: "Skipped due to queued user message.".into(),
        }],
        details: serde_json::Value::Null,
        retention: Retention::Normal,
    };

    tx.send(AgentEvent::ToolExecutionStart {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        args: serde_json::Value::Null,
        preview_command: None,
        is_fanout: false,
        invocation_count: 1,
        parallel: false,
    })
    .ok();

    let result_tokens = context::content_tokens(&result.content);

    tx.send(AgentEvent::ToolExecutionEnd {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        result: result.clone(),
        is_error: true,
        result_tokens,
        duration_ms: 0,
    })
    .ok();

    let msg = Message::ToolResult {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        content: result.content,
        is_error: true,
        timestamp: now_ms(),
        retention: Retention::Normal,
    };

    tx.send(AgentEvent::MessageStart {
        message: msg.clone().into(),
    })
    .ok();
    tx.send(AgentEvent::MessageEnd {
        message: msg.clone().into(),
    })
    .ok();

    msg
}

pub(super) fn skip_tool_call_doom_loop(
    tool_call_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Message {
    let result = ToolResult {
        content: vec![Content::Text {
            text: "Skipped: doom loop detected — repeated identical tool call.".into(),
        }],
        details: serde_json::Value::Null,
        retention: Retention::Normal,
    };

    let preview = build_doom_loop_preview(tool_name, args);

    tx.send(AgentEvent::ToolExecutionStart {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        args: args.clone(),
        preview_command: Some(preview),
        is_fanout: false,
        invocation_count: 1,
        parallel: false,
    })
    .ok();

    let result_tokens = context::content_tokens(&result.content);

    tx.send(AgentEvent::ToolExecutionEnd {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        result: result.clone(),
        is_error: true,
        result_tokens,
        duration_ms: 0,
    })
    .ok();

    let msg = Message::ToolResult {
        tool_call_id: tool_call_id.into(),
        tool_name: tool_name.into(),
        content: result.content,
        is_error: true,
        timestamp: now_ms(),
        retention: Retention::Normal,
    };

    tx.send(AgentEvent::MessageStart {
        message: msg.clone().into(),
    })
    .ok();
    tx.send(AgentEvent::MessageEnd {
        message: msg.clone().into(),
    })
    .ok();

    msg
}

/// Build a compact preview string for doom-loop skipped tool calls.
fn build_doom_loop_preview(tool_name: &str, args: &serde_json::Value) -> String {
    let mut parts = vec![tool_name.to_string()];
    if let serde_json::Value::Object(map) = args {
        for (k, v) in map {
            let val = match v {
                serde_json::Value::String(s) => {
                    if s.len() > 80 {
                        let end = s.floor_char_boundary(80);
                        format!("{}…", &s[..end])
                    } else {
                        s.clone()
                    }
                }
                other => {
                    let s = other.to_string();
                    if s.len() > 80 {
                        let end = s.floor_char_boundary(80);
                        format!("{}…", &s[..end])
                    } else {
                        s
                    }
                }
            };
            parts.push(format!("{k}={val}"));
        }
    }
    parts.join(" ")
}

// ── Spill / truncation helpers ──────────────────────────────────────────

const PREVIEW_CAP: usize = 4_000;

#[derive(Debug)]
struct SpillEvent(SpillProgress);

impl SpillEvent {
    fn to_progress_text(&self) -> String {
        self.0.to_progress_text()
    }
}

async fn process_result(
    spill: &Option<Arc<FsSpill>>,
    tool_call_id: &str,
    tool_name: &str,
    result: ToolResult,
    is_error: bool,
) -> (ToolResult, Option<SpillEvent>) {
    if is_error {
        return (truncate_result(result), None);
    }

    let spill = match spill {
        Some(s) => s,
        None => return (truncate_result(result), None),
    };

    let text = merge_text_blocks(&result.content);
    if text.is_empty() {
        return (result, None);
    }

    let req = crate::spill::SpillRequest {
        key: tool_call_id.to_string(),
        text,
    };

    match spill.spill(req).await {
        Ok(Some(spill_ref)) => {
            let mut details = result.details;
            merge_spill_details(&mut details, &spill_ref);
            let event = SpillEvent(SpillProgress::write(
                spill_ref.path.to_string_lossy(),
                spill_ref.size_bytes,
                spill_ref.preview.len(),
            ));
            (
                build_spilled_result(result.content, details, result.retention, spill_ref),
                Some(event),
            )
        }
        Ok(None) => (truncate_result(result), None),
        Err(e) => {
            tracing::warn!(
                tool_name = tool_name,
                tool_call_id = tool_call_id,
                "spill failed: {e}"
            );
            (truncate_result(result), None)
        }
    }
}

fn build_spilled_result(
    content: Vec<Content>,
    details: serde_json::Value,
    retention: Retention,
    spill_ref: crate::spill::SpillRef,
) -> ToolResult {
    let preview = if spill_ref.preview.len() > PREVIEW_CAP {
        let boundary = spill_ref.preview.floor_char_boundary(PREVIEW_CAP);
        &spill_ref.preview[..boundary]
    } else {
        &spill_ref.preview
    };

    let msg = format!(
        "Tool output was too large ({} bytes) and was saved to:\n{}\n\n\
         Only a preview is shown below. Use Read with offset/limit to read the full output.\n\n\
         Preview:\n{}",
        spill_ref.size_bytes,
        spill_ref.path.display(),
        preview,
    );

    let mut new_content: Vec<Content> = vec![Content::Text { text: msg }];
    for c in content {
        if !matches!(c, Content::Text { .. }) {
            new_content.push(c);
        }
    }

    ToolResult {
        content: new_content,
        details,
        retention,
    }
}

fn merge_spill_details(details: &mut serde_json::Value, spill_ref: &crate::spill::SpillRef) {
    let spill_details = serde_json::json!({
        "kind": "write",
        "path": spill_ref.path.to_string_lossy(),
        "size_bytes": spill_ref.size_bytes,
        "preview_bytes": spill_ref.preview.len(),
    });

    match details {
        serde_json::Value::Object(map) => {
            map.insert("spill".to_string(), spill_details);
        }
        _ => {
            *details = serde_json::json!({ "spill": spill_details });
        }
    }
}

fn truncate_result(result: ToolResult) -> ToolResult {
    let ToolResult {
        content,
        details,
        retention,
    } = result;
    ToolResult {
        content: crate::tools::validation::cap_tool_result_content(
            content,
            crate::tools::validation::MAX_TOOL_RESULT_BYTES,
        ),
        details,
        retention,
    }
}

fn merge_text_blocks(content: &[Content]) -> String {
    let mut merged = String::new();
    for c in content {
        if let Content::Text { text } = c {
            if !merged.is_empty() {
                merged.push('\n');
            }
            merged.push_str(text);
        }
    }
    merged
}

// ── File Read dedup helpers ────────────────────────────────────────────────
