use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bendclaw::writer::BackgroundWriter;

#[tokio::test]
async fn processes_ops_in_order() {
    let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let log2 = log.clone();
    let writer = BackgroundWriter::spawn("test", 16, move |op: u32| {
        let log = log2.clone();
        async move {
            log.lock().await.push(op);
            true
        }
    });

    writer.send(1);
    writer.send(2);
    writer.send(3);
    writer.shutdown().await;

    assert_eq!(*log.lock().await, vec![1, 2, 3]);
}

#[tokio::test]
async fn handler_returning_false_stops_loop() {
    let count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count2 = count.clone();
    let writer = BackgroundWriter::spawn("test", 16, move |_op: u32| {
        let count = count2.clone();
        async move {
            count.fetch_add(1, Ordering::Relaxed);
            false
        }
    });

    writer.send(1);
    writer.send(2);
    writer.send(3);
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(count.load(Ordering::Relaxed), 1);
    writer.shutdown().await;
}

#[tokio::test]
async fn noop_drops_ops_silently() {
    let writer: BackgroundWriter<u32> = BackgroundWriter::noop("test");
    assert!(writer.is_shutting_down());
    writer.send(42);
    writer.shutdown().await;
}

#[tokio::test]
async fn shutdown_is_idempotent() {
    let writer = BackgroundWriter::spawn("test", 4, |_op: u32| async { true });
    writer.shutdown().await;
    writer.shutdown().await;
}

#[tokio::test]
async fn send_after_shutdown_is_dropped() {
    let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let log2 = log.clone();
    let writer = BackgroundWriter::spawn("test", 16, move |op: u32| {
        let log = log2.clone();
        async move {
            log.lock().await.push(op);
            true
        }
    });

    writer.send(1);
    writer.shutdown().await;
    writer.send(2);
    assert_eq!(*log.lock().await, vec![1]);
}
