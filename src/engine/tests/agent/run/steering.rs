//! Steering behavior: user messages injected mid-run and their accounting
//! in `LlmCallStart`.

use evotengine::provider::mock::*;
use evotengine::*;

use crate::fixtures::agent_harness::TestHarness;

// ---------------------------------------------------------------------------
// Steering tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_steering_messages_injected_into_context() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("I see your steering.".into())])
        .steering(vec![AgentMessage::Llm(Message::user("change direction"))])
        .run("Hi")
        .await;

    output.assert_completed();

    // Context should contain: steering msg + user prompt + assistant response
    let user_msgs: Vec<_> = output
        .context_messages
        .iter()
        .filter(|m| m.role() == "user")
        .collect();
    assert_eq!(
        user_msgs.len(),
        2,
        "Expected steering + prompt user messages"
    );
}

#[tokio::test]
async fn test_steering_count_reported_in_llm_call_start() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Got it.".into())])
        .steering(vec![
            AgentMessage::Llm(Message::user("steer 1")),
            AgentMessage::Llm(Message::user("steer 2")),
        ])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty(), "Expected at least one LlmCallStart");
    assert_eq!(counts[0], 2, "Expected 2 injected messages");
}

#[tokio::test]
async fn test_no_steering_reports_zero() {
    let output = TestHarness::new()
        .responses(vec![MockResponse::Text("Hello.".into())])
        .run("Hi")
        .await;

    output.assert_completed();

    let counts = output.injected_counts();
    assert!(!counts.is_empty());
    assert_eq!(counts[0], 0, "Expected 0 injected messages");
}

// ---------------------------------------------------------------------------
// Follow-up delivery — the path background completion notices travel
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_follow_up_message_is_answered_inside_the_same_run() {
    // Completion notices for background tasks are appended to whatever the
    // follow-up queue returns, so this is the path they travel. It fires after
    // the model stops calling tools but before the run settles, which is what
    // makes "carry on, the result will reach you" true: the notice continues the
    // current run rather than waiting for a new one.
    //
    // Two responses, one prompt: a second provider call only happens if the
    // follow-up reopened the run.
    let output = TestHarness::new()
        .responses(vec![
            MockResponse::Text("Started the build.".into()),
            MockResponse::Text("Build finished, carrying on.".into()),
        ])
        .follow_up(vec![AgentMessage::Llm(Message::user(
            "<task-notification><task-id>abc</task-id><status>completed</status></task-notification>",
        ))])
        .run("Run the build")
        .await;

    output.assert_completed();

    let user_texts: Vec<String> = output
        .context_messages
        .iter()
        .filter(|m| m.role() == "user")
        .map(|m| format!("{m:?}"))
        .collect();
    assert!(
        user_texts
            .iter()
            .any(|text| text.contains("task-notification")),
        "notice never reached the context: {user_texts:?}"
    );

    // The model answered after receiving it, rather than the run ending first.
    let assistant_count = output
        .context_messages
        .iter()
        .filter(|m| m.role() == "assistant")
        .count();
    assert_eq!(
        assistant_count, 2,
        "expected a second turn prompted by the follow-up"
    );
}
