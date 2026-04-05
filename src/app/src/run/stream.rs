use serde_json::json;

use crate::run::payload::AssistantBlock;
use crate::run::payload::AssistantPayload;
use crate::run::payload::MessagePayload;
use crate::run::payload::RunFinishedPayload;
use crate::run::payload::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

fn empty_payload() -> serde_json::Value {
    json!({})
}

fn message_payload(message: &str) -> serde_json::Value {
    json!({ "message": message })
}

fn to_payload<T: serde::Serialize>(value: T) -> serde_json::Value {
    match serde_json::to_value(value) {
        Ok(payload) => payload,
        Err(_) => empty_payload(),
    }
}

fn to_message_payload(value: MessagePayload) -> serde_json::Value {
    let fallback = value.message.clone();
    match serde_json::to_value(value) {
        Ok(payload) => payload,
        Err(_) => message_payload(&fallback),
    }
}

fn extract_content_blocks(message: &bend_agent::Message) -> Vec<AssistantBlock> {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            bend_agent::ContentBlock::Text { text } => {
                Some(AssistantBlock::Text { text: text.clone() })
            }
            bend_agent::ContentBlock::ToolUse { id, name, input } => {
                Some(AssistantBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
            }
            bend_agent::ContentBlock::Thinking { thinking, .. } => Some(AssistantBlock::Thinking {
                text: thinking.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub fn map_sdk_message(
    msg: &bend_agent::SDKMessage,
    run_id: &str,
    session_id: &str,
    turn: u32,
) -> RunEvent {
    let (kind, payload) = match msg {
        bend_agent::SDKMessage::System { message } => (
            RunEventKind::System,
            to_message_payload(MessagePayload {
                message: message.clone(),
            }),
        ),
        bend_agent::SDKMessage::Assistant { message, usage } => (
            RunEventKind::AssistantMessage,
            to_payload(AssistantPayload {
                role: format!("{:?}", message.role).to_lowercase(),
                content: extract_content_blocks(message),
                usage: usage
                    .as_ref()
                    .and_then(|value| serde_json::to_value(value).ok()),
            }),
        ),
        bend_agent::SDKMessage::ToolResult {
            tool_use_id,
            tool_name,
            content,
            is_error,
        } => (
            RunEventKind::ToolResult,
            to_payload(ToolResultPayload {
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                content: content.clone(),
                is_error: *is_error,
            }),
        ),
        bend_agent::SDKMessage::Result {
            text,
            usage,
            num_turns,
            cost_usd,
            duration_ms,
            messages,
        } => (
            RunEventKind::RunFinished,
            to_payload(RunFinishedPayload {
                text: text.clone(),
                usage: to_payload(usage),
                num_turns: *num_turns,
                cost_usd: *cost_usd,
                duration_ms: *duration_ms,
                message_count: messages.len(),
            }),
        ),
        bend_agent::SDKMessage::PartialMessage { text } => (
            RunEventKind::PartialMessage,
            to_message_payload(MessagePayload {
                message: text.clone(),
            }),
        ),
        bend_agent::SDKMessage::CompactBoundary { summary } => {
            (RunEventKind::CompactBoundary, json!({ "summary": summary }))
        }
        bend_agent::SDKMessage::Status { message } => (
            RunEventKind::Status,
            to_message_payload(MessagePayload {
                message: message.clone(),
            }),
        ),
        bend_agent::SDKMessage::TaskNotification {
            task_id,
            status,
            message,
        } => (
            RunEventKind::TaskNotification,
            json!({
                "task_id": task_id,
                "status": status,
                "message": message,
            }),
        ),
        bend_agent::SDKMessage::RateLimit {
            retry_after_ms,
            message,
        } => (
            RunEventKind::RateLimit,
            json!({
                "retry_after_ms": retry_after_ms,
                "message": message,
            }),
        ),
        bend_agent::SDKMessage::Progress { message } => (
            RunEventKind::Progress,
            to_message_payload(MessagePayload {
                message: message.clone(),
            }),
        ),
        bend_agent::SDKMessage::Error { message } => (
            RunEventKind::Error,
            to_message_payload(MessagePayload {
                message: message.clone(),
            }),
        ),
    };

    RunEvent::new(
        run_id.to_string(),
        session_id.to_string(),
        turn,
        kind,
        payload,
    )
}

pub fn run_started_event(run_id: &str, session_id: &str) -> RunEvent {
    RunEvent::new(
        run_id.to_string(),
        session_id.to_string(),
        0,
        RunEventKind::RunStarted,
        json!({}),
    )
}
