//! Tests for the ask_user channel bridge — the glue between engine and REPL.
//!
//! These test the channel/oneshot roundtrip without any terminal IO.

use std::sync::Arc;

use bend_engine::tools::AskUserFn;
use bend_engine::tools::AskUserOption;
use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

/// Build the same ask_fn that repl.rs creates, returning the sender half.
fn make_ask_bridge() -> (
    AskUserFn,
    mpsc::UnboundedReceiver<(AskUserRequest, oneshot::Sender<AskUserResponse>)>,
) {
    let (ask_tx, ask_rx) = mpsc::unbounded_channel();
    let ask_fn: AskUserFn = Arc::new(move |request| {
        let tx = ask_tx.clone();
        Box::pin(async move {
            let (resp_tx, resp_rx) = oneshot::channel();
            tx.send((request, resp_tx)).map_err(|e| e.to_string())?;
            resp_rx.await.map_err(|e| e.to_string())
        })
    });
    (ask_fn, ask_rx)
}

fn sample_request() -> AskUserRequest {
    AskUserRequest {
        question: "Which approach?".into(),
        options: vec![
            AskUserOption {
                label: "Option A (Recommended)".into(),
                description: "First choice".into(),
            },
            AskUserOption {
                label: "Option B".into(),
                description: "Second choice".into(),
            },
        ],
    }
}

#[tokio::test]
async fn bridge_selected_roundtrip() {
    let (ask_fn, mut ask_rx) = make_ask_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    // Simulate the REPL side receiving and responding
    let (request, responder) = ask_rx.recv().await.expect("should receive request");
    assert_eq!(request.question, "Which approach?");
    assert_eq!(request.options.len(), 2);
    responder
        .send(AskUserResponse::Selected("Option B".into()))
        .expect("send should succeed");

    let result = handle.await.expect("task should complete");
    assert_eq!(result, Ok(AskUserResponse::Selected("Option B".into())));
}

#[tokio::test]
async fn bridge_custom_roundtrip() {
    let (ask_fn, mut ask_rx) = make_ask_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    let (_request, responder) = ask_rx.recv().await.expect("should receive request");
    responder
        .send(AskUserResponse::Custom("Use SQLite".into()))
        .expect("send should succeed");

    let result = handle.await.expect("task should complete");
    assert_eq!(result, Ok(AskUserResponse::Custom("Use SQLite".into())));
}

#[tokio::test]
async fn bridge_skipped_roundtrip() {
    let (ask_fn, mut ask_rx) = make_ask_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    let (_request, responder) = ask_rx.recv().await.expect("should receive request");
    responder
        .send(AskUserResponse::Skipped)
        .expect("send should succeed");

    let result = handle.await.expect("task should complete");
    assert_eq!(result, Ok(AskUserResponse::Skipped));
}

#[tokio::test]
async fn bridge_responder_dropped_returns_error() {
    let (ask_fn, mut ask_rx) = make_ask_bridge();

    let handle = tokio::spawn(async move { (ask_fn)(sample_request()).await });

    // Receive but drop the responder without sending
    let (_request, _responder) = ask_rx.recv().await.expect("should receive request");
    drop(_responder);

    let result = handle.await.expect("task should complete");
    assert!(result.is_err(), "should error when responder is dropped");
}

#[tokio::test]
async fn bridge_receiver_dropped_returns_error() {
    let (ask_fn, ask_rx) = make_ask_bridge();

    // Drop the receiver before sending a request
    drop(ask_rx);

    let result = (ask_fn)(sample_request()).await;
    assert!(result.is_err(), "should error when receiver is dropped");
}
