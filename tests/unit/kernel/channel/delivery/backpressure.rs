use bendclaw::kernel::channel::delivery::backpressure::BackpressureConfig;
use bendclaw::kernel::channel::delivery::backpressure::BackpressureResult;
use bendclaw::kernel::channel::delivery::backpressure::BackpressureSender;
use bendclaw::kernel::channel::InboundEvent;
use bendclaw::kernel::channel::InboundMessage;

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
    let (tx, _rx) = tokio::sync::mpsc::channel(10);
    let sender = BackpressureSender::new(tx, BackpressureConfig { busy_threshold: 2 });
    assert!(matches!(
        sender.send(make_message("hi")),
        BackpressureResult::Accepted
    ));
}

#[test]
fn busy_when_near_capacity() {
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let sender = BackpressureSender::new(tx, BackpressureConfig { busy_threshold: 3 });
    // First send: remaining=4, above threshold → Accepted.
    assert!(matches!(
        sender.send(make_message("1")),
        BackpressureResult::Accepted
    ));
    // Second send: remaining=3, at threshold → Busy.
    assert!(matches!(
        sender.send(make_message("2")),
        BackpressureResult::Busy
    ));
}

#[test]
fn rejected_when_full() {
    let (tx, _rx) = tokio::sync::mpsc::channel(2);
    let sender = BackpressureSender::new(tx, BackpressureConfig { busy_threshold: 1 });
    sender.send(make_message("1"));
    sender.send(make_message("2"));
    assert!(matches!(
        sender.send(make_message("3")),
        BackpressureResult::Rejected
    ));
}
