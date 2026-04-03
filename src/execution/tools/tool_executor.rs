use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::diagnostics;
use super::parsed_tool_call::DispatchOutcome;
use super::parsed_tool_call::ParsedToolCall;
use super::tool_result::truncate_output;
use super::tool_result::ToolCallResult;
use crate::execution::event::Event;
use crate::execution::skills::parse_skill_args;
use crate::execution::skills::SkillExecutor;
use crate::llm::message::ToolCall;
use crate::tools::definition::tool_definition::ToolDefinition;
use crate::tools::definition::tool_target::ToolTarget;
use crate::tools::definition::toolset::Toolset;
use crate::tools::OpType;
use crate::tools::OperationMeta;
use crate::tools::OperationTracker;
use crate::tools::Tool;
use crate::tools::ToolContext;
use crate::tools::ToolRuntime;

const MAX_PER_TOOL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct CallExecutor {
    definitions: Arc<Vec<ToolDefinition>>,
    bindings: Arc<HashMap<String, ToolTarget>>,
    skill_executor: Arc<dyn SkillExecutor>,
    tool_context: ToolContext,
    cancel: CancellationToken,
    allowed_tool_names: Option<HashSet<String>>,
}

impl CallExecutor {
    /// Build from a `Toolset` — the single runtime boundary.
    pub fn new(
        toolset: &Toolset,
        skill_executor: Arc<dyn SkillExecutor>,
        mut tool_context: ToolContext,
        cancel: CancellationToken,
        event_tx: mpsc::Sender<Event>,
    ) -> Self {
        tool_context.runtime = ToolRuntime {
            event_tx: Some(event_tx),
            cancel: cancel.clone(),
            tool_call_id: None,
        };
        Self {
            definitions: toolset.definitions.clone(),
            bindings: toolset.bindings.clone(),
            skill_executor,
            tool_context,
            cancel,
            allowed_tool_names: toolset.allowed_tool_names.clone(),
        }
    }

    fn resolve_definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.iter().find(|d| d.name == name)
    }

    fn resolve_target(&self, name: &str) -> Option<&ToolTarget> {
        self.bindings.get(name)
    }

    pub fn parse_calls(&self, calls: &[ToolCall]) -> Vec<ParsedToolCall> {
        calls
            .iter()
            .map(|tc| {
                let definition = self.resolve_definition(&tc.name).cloned();
                let target = self.resolve_target(&tc.name).cloned();
                let arguments = match serde_json::from_str(&tc.arguments) {
                    Ok(arguments) => arguments,
                    Err(error) => {
                        diagnostics::log_tool_parse_failed(&tc.name, &tc.id, &tc.arguments, &error);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                };
                ParsedToolCall {
                    call: tc.clone(),
                    arguments,
                    definition,
                    target,
                }
            })
            .collect()
    }

    pub async fn execute_calls(
        &self,
        parsed_calls: &[ParsedToolCall],
        deadline: Instant,
    ) -> Vec<DispatchOutcome> {
        let per_tool_timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(MAX_PER_TOOL_TIMEOUT);

        let futures: Vec<_> = parsed_calls
            .iter()
            .map(|parsed| {
                let parsed = parsed.clone();
                let name = parsed.call.name.clone();
                let args = parsed.arguments.clone();
                let target = parsed.target.clone();
                let fut = self.dispatch(
                    parsed.call.clone(),
                    parsed.arguments.clone(),
                    target.clone(),
                    per_tool_timeout,
                );
                let cancel = self.cancel.clone();
                async move {
                    let tracker =
                        Self::begin_tracker(&name, &args, target.as_ref(), per_tool_timeout);
                    let result = tokio::select! {
                        result = tokio::time::timeout(per_tool_timeout, fut) => {
                            match result {
                                Ok(r) => r,
                                Err(_) => {
                                    diagnostics::log_tool_timed_out(&name, &parsed.call.id);
                                    ToolCallResult::InfraError(
                                        format!("tool '{name}' timed out"),
                                        tracker.finish(),
                                    )
                                }
                            }
                        }
                        _ = cancel.cancelled() => {
                            ToolCallResult::InfraError("cancelled".into(), tracker.finish())
                        }
                    };

                    DispatchOutcome { parsed, result }
                }
            })
            .collect();

        crate::types::runtime::join_bounded(futures, crate::types::runtime::CONCURRENCY_TOOLS).await
    }

    fn begin_tracker(
        name: &str,
        args: &serde_json::Value,
        target: Option<&ToolTarget>,
        timeout: Duration,
    ) -> OperationTracker {
        if let Some(ToolTarget::Builtin(tool)) = target {
            OperationMeta::begin(tool.op_type())
                .maybe_impact(tool.classify_impact(args))
                .timeout(timeout)
                .summary(tool.summarize(args))
        } else {
            OperationMeta::begin(OpType::SkillRun)
                .timeout(timeout)
                .summary(name)
        }
    }

    async fn dispatch(
        &self,
        tc: ToolCall,
        args: serde_json::Value,
        target: Option<ToolTarget>,
        timeout: Duration,
    ) -> ToolCallResult {
        // Enforce tool filter
        if let Some(ref allowed) = self.allowed_tool_names {
            if !allowed.contains(&tc.name) {
                let tracker = Self::begin_tracker(&tc.name, &args, target.as_ref(), timeout);
                return ToolCallResult::InfraError(
                    format!("tool '{}' is not available in this session", tc.name),
                    tracker.finish(),
                );
            }
        }

        match target {
            Some(ToolTarget::Builtin(ref tool)) => self.run_tool(&tc.id, tool, args, timeout).await,
            Some(ToolTarget::Skill) => self.run_skill(&tc.name, &tc.arguments, timeout).await,
            None => {
                let tracker = Self::begin_tracker(&tc.name, &args, None, timeout);
                ToolCallResult::InfraError(
                    format!("tool '{}' not found", tc.name),
                    tracker.finish(),
                )
            }
        }
    }

    async fn run_tool(
        &self,
        tool_call_id: &str,
        tool: &Arc<dyn Tool>,
        args: serde_json::Value,
        timeout: Duration,
    ) -> ToolCallResult {
        let tracker = OperationMeta::begin(tool.op_type())
            .maybe_impact(tool.classify_impact(&args))
            .timeout(timeout)
            .summary(tool.summarize(&args));
        let mut tool_context = self.tool_context.clone();
        tool_context.runtime.tool_call_id = Some(Arc::from(tool_call_id));

        let result = match tool.execute_with_context(args, &tool_context).await {
            Ok(r) => r,
            Err(e) => {
                return ToolCallResult::InfraError(format!("{e}"), tracker.finish());
            }
        };

        let meta = tracker.finish();
        if !result.success {
            let msg = result.error.unwrap_or_else(|| "unknown error".into());
            return ToolCallResult::ToolError(truncate_output(msg), meta);
        }
        ToolCallResult::Success(truncate_output(result.output), meta)
    }

    async fn run_skill(&self, name: &str, arguments: &str, timeout: Duration) -> ToolCallResult {
        let tracker = OperationMeta::begin(OpType::SkillRun)
            .timeout(timeout)
            .summary(name);

        let args = parse_skill_args(name, arguments);
        let out = match self.skill_executor.execute(name, &args).await {
            Ok(out) => out,
            Err(e) => {
                return ToolCallResult::InfraError(format!("{e}"), tracker.finish());
            }
        };

        let meta = tracker.finish();
        if let Some(ref err) = out.error {
            return ToolCallResult::ToolError(truncate_output(err.clone()), meta);
        }
        let text = match out.data {
            Some(serde_json::Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => "OK".into(),
        };
        ToolCallResult::Success(truncate_output(text), meta)
    }
}
