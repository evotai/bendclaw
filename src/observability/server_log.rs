use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::observability::redaction;

#[derive(Debug, Clone, Copy)]
pub struct ServerCtx<'a> {
    pub trace_id: &'a str,
    pub run_id: &'a str,
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub turn: u32,
}

impl<'a> ServerCtx<'a> {
    pub fn new(
        trace_id: &'a str,
        run_id: &'a str,
        session_id: &'a str,
        agent_id: &'a str,
        turn: u32,
    ) -> Self {
        Self {
            trace_id,
            run_id,
            session_id,
            agent_id,
            turn,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServerFields {
    elapsed_ms: u64,
    tokens: u64,
    rows: u64,
    bytes: u64,
    cost: f64,
    attempt: u32,
    payload: Map<String, Value>,
}

impl ServerFields {
    pub fn elapsed_ms(mut self, value: u64) -> Self {
        self.elapsed_ms = value;
        self
    }

    pub fn tokens(mut self, value: u64) -> Self {
        self.tokens = value;
        self
    }

    pub fn rows(mut self, value: u64) -> Self {
        self.rows = value;
        self
    }

    pub fn bytes(mut self, value: u64) -> Self {
        self.bytes = value;
        self
    }

    pub fn cost(mut self, value: f64) -> Self {
        self.cost = value;
        self
    }

    pub fn attempt(mut self, value: u32) -> Self {
        self.attempt = value;
        self
    }

    pub fn detail(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        let key = key.into();
        let value = serde_json::to_value(value)
            .unwrap_or_else(|err| Value::String(format!("<serde-error:{err}>")));
        self.payload.insert(key, value);
        self
    }

    pub fn payload(mut self, value: Value) -> Self {
        match value {
            Value::Object(map) => {
                self.payload.extend(map);
            }
            other => {
                self.payload.insert("payload".to_string(), other);
            }
        }
        self
    }

    fn payload_text(&self) -> String {
        let payload = redaction::redact(Value::Object(self.payload.clone()));
        let text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
        const MAX_PAYLOAD: usize = 1024;
        if text.len() > MAX_PAYLOAD {
            format!("{}...(truncated {} bytes)", &text[..MAX_PAYLOAD], text.len() - MAX_PAYLOAD)
        } else {
            text
        }
    }
}

pub fn info(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::info!(
        trace_id = %ctx.trace_id,
        run_id = %ctx.run_id,
        session_id = %ctx.session_id,
        agent_id = %ctx.agent_id,
        turn = ctx.turn,
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        cost = fields.cost,
        attempt = fields.attempt,
        payload = %fields.payload_text(),
        "server log"
    );
}

pub fn warn(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::warn!(
        trace_id = %ctx.trace_id,
        run_id = %ctx.run_id,
        session_id = %ctx.session_id,
        agent_id = %ctx.agent_id,
        turn = ctx.turn,
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        cost = fields.cost,
        attempt = fields.attempt,
        payload = %fields.payload_text(),
        "server log"
    );
}

pub fn error(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::error!(
        trace_id = %ctx.trace_id,
        run_id = %ctx.run_id,
        session_id = %ctx.session_id,
        agent_id = %ctx.agent_id,
        turn = ctx.turn,
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        cost = fields.cost,
        attempt = fields.attempt,
        payload = %fields.payload_text(),
        "server log"
    );
}
