use std::sync::Arc;

use bendclaw::channels::egress::backpressure::BackpressureConfig;
use bendclaw::channels::egress::backpressure::BackpressureResult;
use bendclaw::channels::egress::backpressure::BackpressureSender;
use bendclaw::channels::model::status::ChannelStatus;
use bendclaw::channels::InboundEvent;
use bendclaw::channels::InboundMessage;

fn make_sender(
    capacity: usize,
    busy_threshold: usize,
) -> (
    BackpressureSender,
    tokio::sync::mpsc::Receiver<InboundEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(capacity);
    let status = Arc::new(ChannelStatus::new());
    let sender = BackpressureSender::new(
        tx,
        BackpressureConfig { busy_threshold },
        status,
        "test-account".to_string(),
    );
    (sender, rx)
}

fn make_message(text: &str) -> InboundEvent {
    InboundEvent::Message(InboundMessage {
        chat_id: "c1".into(),
        sender_id: "s1".into(),
        sender_name: String::new(),
        message_id: String::new(),
        text: text.into(),
        attachments: vec![],
        timestamp: 0,
    })
}

#[test]
fn accepted_when_capacity_available() {
    let (sender, _rx) = make_sender(10, 2);
    assert!(matches!(
        sender.send(make_message("hi")),
        BackpressureResult::Accepted
    ));
}

#[test]
fn busy_when_near_capacity() {
    let (sender, _rx) = make_sender(4, 3);
    assert!(matches!(
        sender.send(make_message("1")),
        BackpressureResult::Accepted
    ));
    assert!(matches!(
        sender.send(make_message("2")),
        BackpressureResult::Busy
    ));
}

#[test]
fn rejected_when_full() {
    let (sender, _rx) = make_sender(2, 1);
    sender.send(make_message("1"));
    sender.send(make_message("2"));
    assert!(matches!(
        sender.send(make_message("3")),
        BackpressureResult::Rejected
    ));
}
