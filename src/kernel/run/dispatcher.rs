use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::kernel::run::event::Event;
use crate::kernel::skills::executor::parse_skill_args;
use crate::kernel::skills::executor::SkillExecutor;
use crate::kernel::tools::registry::ToolRegistry;
use crate::kernel::tools::Tool;
use crate::kernel::tools::ToolContext;
use crate::kernel::tools::ToolRuntime;
use crate::kernel::OpType;
use crate::kernel::OperationMeta;
use crate::kernel::OperationTracker;
use crate::llm::message::ToolCall;
use crate::observability::log::slog;

/// Hard upper limit on any tool/skill output (~64K tokens).
const MAX_TOOL_OUTPUT: usize = 256_000;

/// Semantic result of a single tool/skill call.
#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Success(String, OperationMeta),
    ToolError(String, OperationMeta),
    InfraError(String, OperationMeta),
}

impl ToolCallResult {
    pub fn operation(&self) -> &OperationMeta {
        match self {
            Self::Success(_, meta) | Self::ToolError(_, meta) | Self::InfraError(_, meta) => meta,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DispatchKind {
    Tool,
    Skill,
}

impl DispatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Skill => "skill",
        }
    }
}

const MAX_PER_TOOL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub call: ToolCall,
    pub arguments: serde_json::Value,
    pub kind: DispatchKind,
}

#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub parsed: ParsedToolCall,
    pub result: ToolCallResult,
}

pub struct ToolDispatcher {
    tool_registry: Arc<ToolRegistry>,
    skill_executor: Arc<dyn SkillExecutor>,
    tool_context: ToolContext,
    cancel: CancellationToken,
}

impl ToolDispatcher {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
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
            tool_registry,
            skill_executor,
            tool_context,
            cancel,
        }
    }

    pub fn parse_calls(&self, calls: &[ToolCall]) -> Vec<ParsedToolCall> {
        calls
            .iter()
            .map(|tc| {
                let kind = if self.tool_registry.get(&tc.name).is_some() {
                    DispatchKind::Tool
                } else {
                    DispatchKind::Skill
                };
                let arguments = match serde_json::from_str(&tc.arguments) {
                    Ok(arguments) => arguments,
                    Err(error) => {
                        slog!(warn, "tool", "parse_failed", tool_name = %tc.name, tool_call_id = %tc.id, raw_arguments = %tc.arguments, error = %error,);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                };
                ParsedToolCall {
                    call: tc.clone(),
                    arguments,
                    kind,
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
                let kind = parsed.kind;
                let tool_ref = if matches!(kind, DispatchKind::Tool) {
                    self.tool_registry.get(&name)
                } else {
                    None
                };
                let fut = self.dispatch(
                    parsed.call.clone(),
                    parsed.arguments.clone(),
                    per_tool_timeout,
                );
                let cancel = self.cancel.clone();
                async move {
                    let tracker = Self::begin_tracker(&name, &args, tool_ref, per_tool_timeout);
                    let result = tokio::select! {
                        result = tokio::time::timeout(per_tool_timeout, fut) => {
                            match result {
                                Ok(r) => r,
                                Err(_) => {
                                    slog!(warn, "tool", "timed_out", tool = %name, tool_call_id = %parsed.call.id,);
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

        crate::base::runtime::join_bounded(futures, crate::base::runtime::CONCURRENCY_TOOLS).await
    }

    fn begin_tracker(
        name: &str,
        args: &serde_json::Value,
        tool: Option<&Arc<dyn Tool>>,
        timeout: Duration,
    ) -> OperationTracker {
        if let Some(tool) = tool {
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
        timeout: Duration,
    ) -> ToolCallResult {
        if let Some(tool) = self.tool_registry.get(&tc.name) {
            self.run_tool(&tc.id, &tc.name, tool, args, timeout).await
        } else {
            self.run_skill(&tc.name, &tc.arguments, timeout).await
        }
    }

    async fn run_tool(
        &self,
        tool_call_id: &str,
        name: &str,
        tool: &Arc<dyn Tool>,
        args: serde_json::Value,
        timeout: Duration,
    ) -> ToolCallResult {
        let tracker = Self::begin_tracker(name, &args, Some(tool), timeout);
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

fn truncate_output(text: String) -> String {
    if text.len() <= MAX_TOOL_OUTPUT {
        return text;
    }
    crate::base::truncate_with_notice(&text, MAX_TOOL_OUTPUT)
}
