//! Bounded live delivery, independent of transcript persistence.
//!
//! Overflow is not silent loss: cancel the producer, deliver an explicit error
//! after all accepted events, and reserve a slot for final run statistics. We
//! keep draining the engine for persistence/cleanup rather than blocking it on
//! a consumer that may never read again. No delta/progress reordering or merging.
use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::control::RunControl;
use super::event::RunEvent;
use super::event::RunEventContext;
use super::event::RunEventPayload;

const MAX_PENDING_EVENTS: usize = 256;
const MAX_PENDING_BYTES: usize = 8 * 1024 * 1024;

// Count serialized bytes without copying tool/model data into a JSON buffer.
// The terminal event is reserved separately and may be larger.
struct WireSize(usize);
impl std::io::Write for WireSize {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_PENDING_BYTES {
            return Err(std::io::Error::other("event delivery budget exceeded"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn event_bytes(event: &RunEvent) -> usize {
    // RunEvent's compatibility serializer builds a Value tree; count borrowed
    // fields instead, with fixed headroom for the envelope's JSON field names.
    let mut size = WireSize(256);
    let fields = (
        &event.event_id,
        &event.run_id,
        &event.session_id,
        &event.created_at,
        &event.payload,
    );
    match serde_json::to_writer(&mut size, &fields) {
        Ok(()) => size.0,
        Err(_) => MAX_PENDING_BYTES + 1,
    }
}

struct State {
    events: VecDeque<(RunEvent, usize)>,
    pending_bytes: usize,
    overflowed: bool,
    finished: bool,
    sender_closed: bool,
}
struct Shared {
    state: Mutex<State>,
    ready: Notify,
    receiver_closed: CancellationToken,
    control: RunControl,
}

#[derive(Debug)]
pub struct DeliveryClosed;

pub struct EventSender(Arc<Shared>);
pub struct EventReceiver(Arc<Shared>);

pub fn event_channel(control: RunControl) -> (EventSender, EventReceiver) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            events: VecDeque::new(),
            pending_bytes: 0,
            overflowed: false,
            finished: false,
            sender_closed: false,
        }),
        ready: Notify::new(),
        receiver_closed: CancellationToken::new(),
        control,
    });
    (EventSender(shared.clone()), EventReceiver(shared))
}

impl EventSender {
    pub fn send(&self, event: RunEvent) -> Result<(), DeliveryClosed> {
        let finished = matches!(event.payload, RunEventPayload::RunFinished { .. });
        let bytes = if finished { 0 } else { event_bytes(&event) };
        let mut state = self.0.state.lock();
        if self.0.receiver_closed.is_cancelled() || state.finished {
            return Err(DeliveryClosed);
        }
        if finished {
            state.finished = true;
        } else if state.overflowed {
            return Err(DeliveryClosed);
        } else if state.events.len() >= MAX_PENDING_EVENTS
            || state.pending_bytes.saturating_add(bytes) > MAX_PENDING_BYTES
        {
            state.overflowed = true;
            state.events.push_back((RunEventContext::new(&event.run_id, &event.session_id, event.turn).event(
                RunEventPayload::Error { message: "Run cancelled: event consumer is too slow or output exceeded the delivery budget. Resume the session to read persisted output.".into() },
            ), 0));
            drop(state);
            self.0.control.abort();
            self.0.ready.notify_one();
            return Err(DeliveryClosed);
        }
        state.pending_bytes = state.pending_bytes.saturating_add(bytes);
        state.events.push_back((event, bytes));
        drop(state);
        self.0.ready.notify_one();
        Ok(())
    }

    pub async fn closed(&self) {
        self.0.receiver_closed.cancelled().await;
    }
}
impl Drop for EventSender {
    fn drop(&mut self) {
        self.0.state.lock().sender_closed = true;
        self.0.ready.notify_one();
    }
}
impl EventReceiver {
    pub async fn recv(&mut self) -> Option<RunEvent> {
        loop {
            let ready = self.0.ready.notified();
            {
                let mut state = self.0.state.lock();
                if let Some((event, bytes)) = state.events.pop_front() {
                    state.pending_bytes = state.pending_bytes.saturating_sub(bytes);
                    return Some(event);
                }
                if state.sender_closed {
                    return None;
                }
            }
            ready.await;
        }
    }
}
impl Drop for EventReceiver {
    fn drop(&mut self) {
        self.0.receiver_closed.cancel();
    }
}
