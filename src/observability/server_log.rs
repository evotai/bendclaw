use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::observability::redaction;
use crate::types::truncate_bytes_on_char_boundary;

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

    /// Build a compact payload string from the detail map (redacted).
    fn format_payload(&self) -> String {
        let redacted = redaction::redact(Value::Object(self.payload.clone()));
        let mut buf = String::new();
        if let Value::Object(map) = redacted {
            for (k, v) in &map {
                match v {
                    Value::Null => {}
                    Value::Bool(b) => {
                        write!(buf, " {k}={b}").ok();
                    }
                    Value::Number(n) => {
                        write!(buf, " {k}={n}").ok();
                    }
                    Value::String(s) => {
                        if !s.is_empty() {
                            const MAX_VAL: usize = 200;
                            if s.len() > MAX_VAL {
                                write!(
                                    buf,
                                    " {k}=\"{}...\"",
                                    truncate_bytes_on_char_boundary(s, MAX_VAL)
                                )
                                .ok();
                            } else if s.contains(' ') {
                                write!(buf, " {k}=\"{s}\"").ok();
                            } else {
                                write!(buf, " {k}={s}").ok();
                            }
                        }
                    }
                    _ => {
                        let json = serde_json::to_string(v).unwrap_or_default();
                        const MAX_JSON: usize = 200;
                        if json.len() > MAX_JSON {
                            write!(
                                buf,
                                " {k}={}...",
                                truncate_bytes_on_char_boundary(&json, MAX_JSON)
                            )
                            .ok();
                        } else {
                            write!(buf, " {k}={json}").ok();
                        }
                    }
                }
            }
        }
        buf
    }

    /// Build a compact single-line message: non-zero metrics + flattened payload + context.
    #[allow(dead_code)]
    fn format_line(&self, stage: &str, status: &str, ctx: &ServerCtx<'_>) -> String {
        let mut buf = format!("[{stage}] {status}");

        if self.elapsed_ms > 0 {
            write!(buf, " elapsed={}ms", self.elapsed_ms).ok();
        }
        if self.tokens > 0 {
            write!(buf, " tokens={}", self.tokens).ok();
        }
        if self.rows > 0 {
            write!(buf, " rows={}", self.rows).ok();
        }
        if self.bytes > 0 {
            write!(buf, " bytes={}", self.bytes).ok();
        }
        if self.cost > 0.0 {
            write!(buf, " cost={:.4}", self.cost).ok();
        }
        if self.attempt > 0 {
            write!(buf, " attempt={}", self.attempt).ok();
        }

        // Flatten payload key=value pairs inline.
        let redacted = redaction::redact(Value::Object(self.payload.clone()));
        if let Value::Object(map) = redacted {
            for (k, v) in &map {
                match v {
                    Value::Null => {}
                    Value::Bool(b) => {
                        write!(buf, " {k}={b}").ok();
                    }
                    Value::Number(n) => {
                        write!(buf, " {k}={n}").ok();
                    }
                    Value::String(s) => {
                        if !s.is_empty() {
                            const MAX_VAL: usize = 200;
                            if s.len() > MAX_VAL {
                                write!(
                                    buf,
                                    " {k}=\"{}...\"",
                                    truncate_bytes_on_char_boundary(s, MAX_VAL)
                                )
                                .ok();
                            } else if s.contains(' ') {
                                write!(buf, " {k}=\"{s}\"").ok();
                            } else {
                                write!(buf, " {k}={s}").ok();
                            }
                        }
                    }
                    _ => {
                        let json = serde_json::to_string(v).unwrap_or_default();
                        const MAX_JSON: usize = 200;
                        if json.len() > MAX_JSON {
                            write!(
                                buf,
                                " {k}={}...",
                                truncate_bytes_on_char_boundary(&json, MAX_JSON)
                            )
                            .ok();
                        } else {
                            write!(buf, " {k}={json}").ok();
                        }
                    }
                }
            }
        }

        // Context IDs after separator.
        write!(
            buf,
            " | run={} sid={} turn={}",
            ctx.run_id, ctx.session_id, ctx.turn
        )
        .ok();

        buf
    }
}

/// Return the first 8 characters of a run ID for compact log display.
pub fn short_run_id(run_id: &str) -> &str {
    let end = run_id.len().min(8);
    // Ensure we don't split a multi-byte char (run IDs are ASCII, but be safe).
    &run_id[..end]
}

pub fn preview_text(text: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 200;
    let preview: String = text.chars().take(MAX_PREVIEW_CHARS).collect();
    if text.chars().count() > MAX_PREVIEW_CHARS {
        format!("{preview}...")
    } else {
        preview
    }
}

pub fn info(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::info!(
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        payload = %fields.format_payload(),
        run_id = ctx.run_id,
        session_id = ctx.session_id,
        agent_id = ctx.agent_id,
        turn = ctx.turn,
        "{stage} {status}"
    );
}

pub fn warn(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::warn!(
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        payload = %fields.format_payload(),
        run_id = ctx.run_id,
        session_id = ctx.session_id,
        agent_id = ctx.agent_id,
        turn = ctx.turn,
        "{stage} {status}"
    );
}

pub fn error(ctx: &ServerCtx<'_>, stage: &str, status: &str, fields: ServerFields) {
    tracing::error!(
        stage,
        status,
        elapsed_ms = fields.elapsed_ms,
        tokens = fields.tokens,
        rows = fields.rows,
        bytes = fields.bytes,
        payload = %fields.format_payload(),
        run_id = ctx.run_id,
        session_id = ctx.session_id,
        agent_id = ctx.agent_id,
        turn = ctx.turn,
        "{stage} {status}"
    );
}
