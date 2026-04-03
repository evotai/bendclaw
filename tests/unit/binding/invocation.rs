//! Tests for the invocation layer: request validation.

use bendclaw::request::invocation::*;
use bendclaw::request::validate;
use bendclaw::sessions::runtime::run_options::RunOptions;

fn make_request(agent_id: &str, user_id: &str) -> InvocationRequest {
    InvocationRequest {
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
        context: ConversationContext::None,
        prompt: "hello".into(),
        options: RunOptions::default(),
        session_options: SessionBuildOptions::default(),
    }
}

#[test]
fn empty_agent_id_is_rejected() {
    let req = make_request("", "cli");
    assert!(validate(&req).is_err());
}

#[test]
fn empty_user_id_is_rejected() {
    let req = make_request("local", "");
    assert!(validate(&req).is_err());
}

#[test]
fn valid_request_passes() {
    let req = make_request("a1", "u1");
    assert!(validate(&req).is_ok());
}
