use serde::Deserialize;
use serde::Serialize;

use crate::base::Content;
use crate::base::ErrorSource;
use crate::base::Role;
use crate::base::ToolCall;
use crate::kernel::tools::operation::OpType;
use crate::kernel::tools::operation::OperationMeta;

/// Per-message token and timing metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageMetrics {
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub reasoning_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub ttft_ms: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub duration_ms: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: Vec<Content>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin_run_id: Option<String>,
    },
    Assistant {
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin_run_id: Option<String>,
        operation: OperationMeta,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metrics: Option<MessageMetrics>,
    },
    ToolResult {
        tool_call_id: String,
        name: String,
        output: String,
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin_run_id: Option<String>,
        operation: OperationMeta,
    },
    Memory {
        operation: String,
        key: String,
        value: String,
    },
    CompactionSummary {
        summary: String,
        operation: OperationMeta,
    },
    Note {
        text: String,
    },
    OperationEvent {
        kind: String,
        name: String,
        status: String,
        detail: serde_json::Value,
    },
    Error {
        source: ErrorSource,
        message: String,
    },
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self::System {
            content: text.into(),
        }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self::User {
            content: vec![Content::text(text)],
            origin_run_id: None,
        }
    }
    pub fn user_multimodal(parts: Vec<Content>) -> Self {
        Self::User {
            content: parts,
            origin_run_id: None,
        }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::Assistant {
            content: text.into(),
            tool_calls: Vec::new(),
            origin_run_id: None,
            operation: OperationMeta::new(OpType::Reasoning),
            metrics: None,
        }
    }
    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self::Assistant {
            content: text.into(),
            tool_calls,
            origin_run_id: None,
            operation: OperationMeta::new(OpType::Reasoning),
            metrics: None,
        }
    }
    pub fn assistant_with_operation(
        text: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        operation: OperationMeta,
    ) -> Self {
        Self::Assistant {
            content: text.into(),
            tool_calls,
            origin_run_id: None,
            operation,
            metrics: None,
        }
    }
    pub fn assistant_with_metrics(
        text: impl Into<String>,
        tool_calls: Vec<ToolCall>,
        operation: OperationMeta,
        metrics: MessageMetrics,
    ) -> Self {
        Self::Assistant {
            content: text.into(),
            tool_calls,
            origin_run_id: None,
            operation,
            metrics: Some(metrics),
        }
    }
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        output: impl Into<String>,
        success: bool,
    ) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            output: output.into(),
            success,
            origin_run_id: None,
            operation: OperationMeta::new(OpType::Execute),
        }
    }
    pub fn tool_result_with_operation(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        output: impl Into<String>,
        success: bool,
        operation: OperationMeta,
    ) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            output: output.into(),
            success,
            origin_run_id: None,
            operation,
        }
    }
    pub fn compaction(summary: impl Into<String>) -> Self {
        Self::CompactionSummary {
            summary: summary.into(),
            operation: OperationMeta::new(OpType::Compaction),
        }
    }
    pub fn compaction_with_operation(summary: impl Into<String>, operation: OperationMeta) -> Self {
        Self::CompactionSummary {
            summary: summary.into(),
            operation,
        }
    }
    pub fn note(text: impl Into<String>) -> Self {
        Self::Note { text: text.into() }
    }
    pub fn operation_event(
        kind: impl Into<String>,
        name: impl Into<String>,
        status: impl Into<String>,
        detail: serde_json::Value,
    ) -> Self {
        Self::OperationEvent {
            kind: kind.into(),
            name: name.into(),
            status: status.into(),
            detail,
        }
    }
    pub fn error(source: ErrorSource, msg: impl Into<String>) -> Self {
        Self::Error {
            source,
            message: msg.into(),
        }
    }
    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        let run_id = Some(run_id.into());
        match &mut self {
            Self::User { origin_run_id, .. } => *origin_run_id = run_id,
            Self::Assistant { origin_run_id, .. } => *origin_run_id = run_id,
            Self::ToolResult { origin_run_id, .. } => *origin_run_id = run_id,
            _ => {}
        }
        self
    }
    pub fn origin_run_id(&self) -> Option<&str> {
        match self {
            Self::User { origin_run_id, .. }
            | Self::Assistant { origin_run_id, .. }
            | Self::ToolResult { origin_run_id, .. } => origin_run_id.as_deref(),
            _ => None,
        }
    }
    pub fn role(&self) -> Option<Role> {
        match self {
            Self::System { .. } | Self::CompactionSummary { .. } => Some(Role::System),
            Self::User { .. } => Some(Role::User),
            Self::Assistant { .. } | Self::Error { .. } => Some(Role::Assistant),
            Self::ToolResult { .. } => Some(Role::Tool),
            Self::Memory { .. } | Self::Note { .. } | Self::OperationEvent { .. } => None,
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::System { content } => content.clone(),
            Self::User { content, .. } => content
                .iter()
                .filter_map(|c| match c {
                    Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            Self::Assistant { content, .. } => content.clone(),
            Self::ToolResult { output, .. } => output.clone(),
            Self::CompactionSummary { summary, .. } => summary.clone(),
            Self::Memory { key, value, .. } => format!("{key}: {value}"),
            Self::Note { text } => text.clone(),
            Self::OperationEvent {
                kind,
                name,
                status,
                detail,
            } => format!("[{kind}:{name}] {status} - {detail}"),
            Self::Error { source, message } => format!("[{source}] {message}"),
        }
    }
}
