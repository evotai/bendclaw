use bendclaw::kernel::run::fmt::to_chat_messages;
use bendclaw::planning::prompt_projection::count_prompt_tokens;
use bendclaw::planning::prompt_projection::is_prompt_relevant;
use bendclaw::planning::prompt_projection::prompt_token_vec;
use bendclaw::sessions::Message;

#[test]
fn system_is_prompt_relevant() {
    assert!(is_prompt_relevant(&Message::system("hello")));
}

#[test]
fn user_is_prompt_relevant() {
    assert!(is_prompt_relevant(&Message::user("hello")));
}

#[test]
fn assistant_is_prompt_relevant() {
    assert!(is_prompt_relevant(&Message::assistant("hello")));
}

#[test]
fn tool_result_is_prompt_relevant() {
    assert!(is_prompt_relevant(&Message::tool_result(
        "tc-1", "shell", "output", true
    )));
}

#[test]
fn compaction_summary_is_prompt_relevant() {
    assert!(is_prompt_relevant(&Message::compaction("summary")));
}

#[test]
fn note_is_not_prompt_relevant() {
    assert!(!is_prompt_relevant(&Message::note("some note")));
}

#[test]
fn operation_event_is_not_prompt_relevant() {
    assert!(!is_prompt_relevant(&Message::operation_event(
        "llm",
        "reasoning.turn",
        "started",
        serde_json::json!({}),
    )));
}

#[test]
fn memory_is_not_prompt_relevant() {
    let msg = Message::Memory {
        operation: "extract".into(),
        key: "key".into(),
        value: "value".into(),
    };
    assert!(!is_prompt_relevant(&msg));
}

#[test]
fn count_prompt_tokens_excludes_non_relevant() {
    let messages = vec![
        Message::user("hello world"),
        Message::Memory {
            operation: "extract".into(),
            key: "key".into(),
            value: "value".into(),
        },
        Message::note("internal note"),
        Message::operation_event("llm", "turn", "done", serde_json::json!({})),
        Message::assistant("response"),
    ];
    let total = count_prompt_tokens(&messages);
    let expected = bendclaw::llm::tokens::count_tokens("hello world")
        + bendclaw::llm::tokens::count_tokens("response");
    assert_eq!(total, expected);
}

#[test]
fn prompt_token_vec_zeros_non_relevant() {
    let messages = vec![
        Message::user("hello"),
        Message::Memory {
            operation: "extract".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::assistant("world"),
    ];
    let vec = prompt_token_vec(&messages);
    assert_eq!(vec.len(), 3);
    assert!(vec[0] > 0); // user
    assert_eq!(vec[1], 0); // memory
    assert!(vec[2] > 0); // assistant
}

/// Ensures is_prompt_relevant() and to_chat_messages() agree on which
/// messages are filtered. If these ever drift, compaction budget and
/// actual LLM request will diverge.
#[test]
fn prompt_projection_consistent_with_to_chat_messages() {
    let messages = vec![
        Message::system("system"),
        Message::user("user"),
        Message::assistant("assistant"),
        Message::tool_result("tc-1", "shell", "output", true),
        Message::compaction("summary"),
        Message::Memory {
            operation: "extract".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::note("note"),
        Message::operation_event("llm", "turn", "done", serde_json::json!({})),
    ];

    let relevant_count = messages.iter().filter(|m| is_prompt_relevant(m)).count();
    let chat_count = to_chat_messages(&messages).len();

    assert_eq!(
        relevant_count, chat_count,
        "is_prompt_relevant ({relevant_count}) and to_chat_messages ({chat_count}) disagree on message count"
    );

    // Also verify per-message: every message that produces a ChatMessage
    // must be prompt-relevant, and vice versa.
    for msg in &messages {
        let produces_chat = !to_chat_messages(std::slice::from_ref(msg)).is_empty();
        let is_relevant = is_prompt_relevant(msg);
        assert_eq!(
            is_relevant,
            produces_chat,
            "disagreement on {:?}: is_prompt_relevant={is_relevant}, produces_chat={produces_chat}",
            std::mem::discriminant(msg)
        );
    }
}
