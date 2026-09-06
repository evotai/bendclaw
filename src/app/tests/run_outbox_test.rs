use evot::agent::run::outbox::event_channel;
use evot::agent::RunControl;
use evot::agent::RunEventContext;
use evot::agent::RunEventPayload;

#[tokio::test]
async fn slow_consumer_is_cancelled_with_error_and_reserved_terminal_event() {
    let control = RunControl::new();
    let (tx, mut rx) = event_channel(control.clone());
    let context = RunEventContext::new("run", "session", 1);
    let mut accepted = 0;
    for _ in 0..1000 {
        if tx.send(context.started()).is_ok() {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 256);
    assert!(control.is_cancelled());
    assert!(tx
        .send(context.finished("done".into(), Default::default(), 1, 1, 0, vec![]))
        .is_ok());
    assert!(tx.send(context.started()).is_err());
    drop(tx);
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    assert_eq!(events.len(), 258);
    assert!(events[..256]
        .iter()
        .all(|event| matches!(event.payload, RunEventPayload::RunStarted { .. })));
    assert!(
        matches!(&events[256].payload, RunEventPayload::Error { message } if message.contains("consumer is too slow"))
    );
    assert!(matches!(
        events[257].payload,
        RunEventPayload::RunFinished { .. }
    ));
}

#[tokio::test]
async fn byte_budget_is_bounded_and_released_when_drained() {
    let context = RunEventContext::new("r", "s", 1);
    let control = RunControl::new();
    let (tx, mut rx) = event_channel(control.clone());
    let large = || {
        context.event(RunEventPayload::Error {
            message: "x".repeat(5 * 1024 * 1024),
        })
    };
    assert!(tx.send(large()).is_ok());
    assert!(rx.recv().await.is_some());
    assert!(tx.send(large()).is_ok());
    assert!(!control.is_cancelled());
    assert!(tx.send(large()).is_err());
    assert!(control.is_cancelled());
    drop(tx);
    assert!(rx.recv().await.is_some());
    assert!(
        matches!(rx.recv().await.map(|e| e.payload), Some(RunEventPayload::Error { message }) if message.contains("delivery budget"))
    );
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn drained_capacity_is_reused_without_cancellation() {
    let control = RunControl::new();
    let (tx, mut rx) = event_channel(control.clone());
    let context = RunEventContext::new("run", "session", 1);
    for _ in 0..1000 {
        assert!(tx.send(context.started()).is_ok());
        assert!(rx.recv().await.is_some());
    }
    assert!(!control.is_cancelled());
    drop(tx);
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn receiver_drop_releases_sender_and_pending_read_wakes_on_sender_drop() {
    let (tx, rx) = event_channel(RunControl::new());
    drop(rx);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), tx.closed())
            .await
            .is_ok()
    );
    assert!(tx
        .send(RunEventContext::new("r", "s", 1).started())
        .is_err());
    let (tx, mut rx) = event_channel(RunControl::new());
    let reader = tokio::spawn(async move { rx.recv().await });
    tokio::task::yield_now().await;
    drop(tx);
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), reader).await;
    assert!(matches!(result, Ok(Ok(None))));
}
