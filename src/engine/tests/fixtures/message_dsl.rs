use evotengine::context::*;
use evotengine::types::*;

/// Converts a condensed string pattern into a Vec<AgentMessage>.
///
/// # Pattern Format
/// - `u` = User message
/// - `a` = Assistant text message
/// - `t` = Assistant message with tool call (must be followed by `r`)
/// - `T` = Assistant message with tool call (orphan — no matching `r`)
/// - `r` = Tool result (auto-matches most recent unmatched `t`)
/// - spaces are ignored (for readability)
///
/// # Examples
/// ```rust,ignore
/// let msgs = pat("u a u").build();
/// let msgs = pat("u tr u tr u").pad(2000).build();
/// let msgs = pat("u tr").tool_output(5000).build();
/// let msgs = pat("u T u T u").build();  // orphan tool calls
/// ```
#[derive(Debug, Clone)]
pub struct MessagePattern {
    pattern: String,
    pad_chars: usize,
    tool_output_chars: usize,
}

impl MessagePattern {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            pad_chars: 10,
            tool_output_chars: 10,
        }
    }

    pub fn pad(mut self, chars: usize) -> Self {
        self.pad_chars = chars;
        self
    }

    pub fn tool_output(mut self, chars: usize) -> Self {
        self.tool_output_chars = chars;
        self
    }

    pub fn build(self) -> Vec<AgentMessage> {
        let chars: Vec<char> = self.pattern.chars().filter(|c| *c != ' ').collect();
        let mut messages = Vec::new();
        let mut msg_index = 0usize;
        let mut tool_id_counter = 0usize;
        let mut pending_tool_ids: Vec<String> = Vec::new();

        for ch in &chars {
            match ch {
                'u' => {
                    let text = format!("msg-{} {}", msg_index, "x".repeat(self.pad_chars));
                    messages.push(AgentMessage::Llm(Message::user(&text)));
                    msg_index += 1;
                }
                'a' => {
                    let text = format!("msg-{} {}", msg_index, "x".repeat(self.pad_chars));
                    messages.push(AgentMessage::Llm(Message::Assistant {
                        content: vec![Content::Text { text }],
                        stop_reason: StopReason::Stop,
                        model: "test".into(),
                        provider: "test".into(),
                        usage: Usage::default(),
                        timestamp: 0,
                        error_message: None,
                        response_id: None,
                    }));
                    msg_index += 1;
                }
                't' => {
                    tool_id_counter += 1;
                    let id = format!("tc-{}", tool_id_counter);
                    pending_tool_ids.push(id.clone());
                    messages.push(AgentMessage::Llm(Message::Assistant {
                        content: vec![Content::ToolCall {
                            id,
                            name: "bash".into(),
                            arguments: serde_json::json!({}),
                        }],
                        stop_reason: StopReason::ToolUse,
                        model: "test".into(),
                        provider: "test".into(),
                        usage: Usage::default(),
                        timestamp: 0,
                        error_message: None,
                        response_id: None,
                    }));
                    msg_index += 1;
                }
                'T' => {
                    // Orphan tool call — no matching 'r' expected
                    tool_id_counter += 1;
                    let id = format!("tc-{}", tool_id_counter);
                    messages.push(AgentMessage::Llm(Message::Assistant {
                        content: vec![Content::ToolCall {
                            id,
                            name: "bash".into(),
                            arguments: serde_json::json!({}),
                        }],
                        stop_reason: StopReason::ToolUse,
                        model: "test".into(),
                        provider: "test".into(),
                        usage: Usage::default(),
                        timestamp: 0,
                        error_message: None,
                        response_id: None,
                    }));
                    msg_index += 1;
                }
                'r' => {
                    let id = pending_tool_ids
                        .pop()
                        .unwrap_or_else(|| panic!("pattern error: 'r' without matching 't'"));
                    messages.push(AgentMessage::Llm(Message::ToolResult {
                        tool_call_id: id,
                        tool_name: "bash".into(),
                        content: vec![Content::Text {
                            text: "r".repeat(self.tool_output_chars),
                        }],
                        is_error: false,
                        timestamp: 0,
                        retention: Retention::Normal,
                    }));
                    msg_index += 1;
                }
                other => panic!("pattern error: unknown character '{}'", other),
            }
        }

        if !pending_tool_ids.is_empty() {
            panic!(
                "pattern error: {} unmatched 't' without 'r': {:?}",
                pending_tool_ids.len(),
                pending_tool_ids
            );
        }

        messages
    }
}

/// Shorthand for `MessagePattern::new(pattern)`.
pub fn pat(pattern: &str) -> MessagePattern {
    MessagePattern::new(pattern)
}

// ---------------------------------------------------------------------------
// DSL self-tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_user() {
        let msgs = pat("u").build();
        assert_eq!(msgs.len(), 1);
        assert!(matches!(&msgs[0], AgentMessage::Llm(Message::User { .. })));
    }

    #[test]
    fn test_user_assistant_user() {
        let msgs = pat("uau").build();
        assert_eq!(msgs.len(), 3);
        assert!(matches!(&msgs[0], AgentMessage::Llm(Message::User { .. })));
        assert!(matches!(
            &msgs[1],
            AgentMessage::Llm(Message::Assistant { .. })
        ));
        assert!(matches!(&msgs[2], AgentMessage::Llm(Message::User { .. })));
    }

    #[test]
    fn test_tool_turn() {
        let msgs = pat("utr").build();
        assert_eq!(msgs.len(), 3);
        // t and r should have matching tool_call_id
        if let AgentMessage::Llm(Message::Assistant { content, .. }) = &msgs[1] {
            if let Content::ToolCall { id, .. } = &content[0] {
                if let AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) = &msgs[2] {
                    assert_eq!(id, tool_call_id);
                } else {
                    panic!("expected tool result");
                }
            } else {
                panic!("expected tool call");
            }
        } else {
            panic!("expected assistant");
        }
    }

    #[test]
    fn test_multiple_tool_turns_unique_ids() {
        let msgs = pat("u tr tr u").build();
        assert_eq!(msgs.len(), 6);
        // Extract tool_call_ids
        let mut ids = Vec::new();
        for msg in &msgs {
            if let AgentMessage::Llm(Message::ToolResult { tool_call_id, .. }) = msg {
                ids.push(tool_call_id.clone());
            }
        }
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "tool_call_ids should be unique");
    }

    #[test]
    fn test_spaces_ignored() {
        let msgs1 = pat("utr").build();
        let msgs2 = pat("u t r").build();
        assert_eq!(msgs1.len(), msgs2.len());
    }

    #[test]
    fn test_pad_controls_size() {
        let small = pat("u").pad(10).build();
        let large = pat("u").pad(5000).build();
        let small_tok = total_tokens(&small);
        let large_tok = total_tokens(&large);
        assert!(large_tok > small_tok * 5);
    }

    #[test]
    fn test_tool_output_controls_size() {
        let small = pat("utr").tool_output(10).build();
        let large = pat("utr").tool_output(5000).build();
        let small_tok = total_tokens(&small);
        let large_tok = total_tokens(&large);
        assert!(large_tok > small_tok * 5);
    }

    #[test]
    #[should_panic(expected = "pattern error")]
    fn test_orphan_r_panics() {
        pat("r").build();
    }

    #[test]
    #[should_panic(expected = "pattern error")]
    fn test_unmatched_t_panics() {
        pat("ut").build();
    }

    #[test]
    #[should_panic(expected = "pattern error")]
    fn test_invalid_char_panics() {
        pat("uxr").build();
    }
}
