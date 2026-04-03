use std::time::Duration;

use super::engine_state::RunLoopState;
use super::response_mapper::LLMResponse;
use crate::llm::message::ToolCall;
use crate::sessions::message::MessageMetrics;
use crate::sessions::Message;
use crate::tools::OpType;
use crate::tools::OperationMeta;

pub fn assistant_message_from_turn(
    turn: &LLMResponse,
    model: &str,
    max_duration: Duration,
    run_id: &str,
) -> Message {
    let ttft_ms = turn.ttft_ms().unwrap_or(0);
    let reasoning_meta = OperationMeta::begin(OpType::Reasoning)
        .timeout(max_duration)
        .summary(format!("{model} -> {} tokens", turn.usage().total_tokens))
        .finish();

    let metrics = MessageMetrics {
        input_tokens: turn.usage().prompt_tokens,
        output_tokens: turn.usage().completion_tokens,
        reasoning_tokens: 0,
        ttft_ms,
        duration_ms: 0,
    };

    let tool_calls = if turn.has_tool_calls() {
        turn.tool_calls().to_vec()
    } else {
        Vec::new()
    };

    Message::assistant_with_metrics(turn.text(), tool_calls, reasoning_meta, metrics)
        .with_run_id(run_id)
}

pub fn record_assistant_turn(
    messages: &mut Vec<Message>,
    turn: &LLMResponse,
    state: &mut RunLoopState,
    model: &str,
    max_duration: Duration,
    run_id: &str,
) {
    messages.push(assistant_message_from_turn(
        turn,
        model,
        max_duration,
        run_id,
    ));
    if !turn.has_tool_calls() {
        state.record_final_response(turn.content_blocks());
    }
}

pub fn aborted_tool_result_messages(tool_calls: &[ToolCall], run_id: &str) -> Vec<Message> {
    tool_calls
        .iter()
        .map(|tool_call| {
            Message::tool_result(&tool_call.id, &tool_call.name, "aborted", false)
                .with_run_id(run_id)
        })
        .collect()
}
