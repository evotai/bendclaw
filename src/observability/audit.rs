use serde_json::Map;
use serde_json::Value;

use crate::kernel::run::event::Event;
use crate::observability::redaction;
use crate::observability::server_log::ServerCtx;

pub fn event(name: impl Into<String>, payload: Value) -> Event {
    Event::Audit {
        name: name.into(),
        payload: redaction::redact(payload),
    }
}

pub fn event_from_map(name: impl Into<String>, payload: Map<String, Value>) -> Event {
    event(name, Value::Object(payload))
}

pub fn base_payload(ctx: &ServerCtx<'_>) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert(
        "trace_id".to_string(),
        Value::String(ctx.trace_id.to_string()),
    );
    payload.insert("run_id".to_string(), Value::String(ctx.run_id.to_string()));
    payload.insert(
        "session_id".to_string(),
        Value::String(ctx.session_id.to_string()),
    );
    payload.insert(
        "agent_id".to_string(),
        Value::String(ctx.agent_id.to_string()),
    );
    payload.insert("turn".to_string(), Value::from(ctx.turn));
    payload
}
