use evotengine::provider::stream_sink::StreamSink;
use evotengine::provider::StreamEvent;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn bounded_delivery_waits_for_capacity_and_cancel_releases_waiter(
) -> Result<(), Box<dyn std::error::Error>> {
    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let sink = StreamSink::Bounded {
        tx,
        cancel: cancel.clone(),
    };
    assert!(sink.send(StreamEvent::Start).await.is_ok());
    let pending = sink.send(StreamEvent::Start);
    tokio::pin!(pending);
    assert!(futures::poll!(&mut pending).is_pending());
    assert!(rx.recv().await.is_some());
    assert!(pending.await.is_ok());
    let blocked = sink.send(StreamEvent::Start);
    tokio::pin!(blocked);
    assert!(futures::poll!(&mut blocked).is_pending());
    cancel.cancel();
    assert!(blocked.await.is_err());
    Ok(())
}

#[tokio::test]
async fn receiver_closure_cancels_provider() {
    let cancel = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let sink = StreamSink::Bounded {
        tx,
        cancel: cancel.clone(),
    };
    drop(rx);
    assert!(sink.send(StreamEvent::Start).await.is_err());
    assert!(cancel.is_cancelled());
}
