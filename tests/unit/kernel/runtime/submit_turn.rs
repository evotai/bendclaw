use bendclaw::kernel::runtime::SubmitResult;
use bendclaw::storage::pool::QueryResponse;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn noop_fake() -> FakeDatabend {
    FakeDatabend::new(|_sql, _database| {
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    })
}

#[tokio::test]
async fn submit_turn_clear_returns_control_message() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", "/clear", "t1", None, "", "", false)
        .await
        .expect("submit_turn /clear should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert_eq!(message, "Conversation history cleared.");
        }
        _ => panic!("expected SubmitResult::Control for /clear"),
    }
}

#[tokio::test]
async fn submit_turn_clear_case_insensitive() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", " /CLEAR ", "t1", None, "", "", false)
        .await
        .expect("submit_turn /CLEAR should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert_eq!(message, "Conversation history cleared.");
        }
        _ => panic!("expected SubmitResult::Control for /CLEAR"),
    }
}

#[tokio::test]
async fn submit_turn_new_returns_control_message() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", "/new", "t1", None, "", "", false)
        .await
        .expect("submit_turn /new should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert_eq!(message, "New conversation started.");
        }
        _ => panic!("expected SubmitResult::Control for /new"),
    }
}

#[tokio::test]
async fn submit_turn_new_case_insensitive() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", " /NEW ", "t1", None, "", "", false)
        .await
        .expect("submit_turn /NEW should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert_eq!(message, "New conversation started.");
        }
        _ => panic!("expected SubmitResult::Control for /NEW"),
    }
}

#[tokio::test]
async fn submit_turn_cancel_still_works() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", "cancel", "t1", None, "", "", false)
        .await
        .expect("submit_turn cancel should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert_eq!(message, "Run cancelled.");
        }
        _ => panic!("expected SubmitResult::Control for cancel"),
    }
}

#[tokio::test]
async fn submit_turn_status_still_works() {
    let runtime = test_runtime(noop_fake());
    let result = runtime
        .submit_turn("a1", "s1", "u1", "status", "t1", None, "", "", false)
        .await
        .expect("submit_turn status should succeed");

    match result {
        SubmitResult::Control { message } => {
            assert!(message.contains("No active session."));
        }
        _ => panic!("expected SubmitResult::Control for status"),
    }
}
