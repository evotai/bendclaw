use std::sync::Arc;

use async_trait::async_trait;

use crate::cli::args::OutputFormat;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
use crate::request::MessagePayload;
use crate::request::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

pub struct TextSink;

pub struct StreamJsonSink;

pub fn create_sink(format: &OutputFormat) -> Arc<dyn EventSink> {
    match format {
        OutputFormat::Text => Arc::new(TextSink),
        OutputFormat::StreamJson => Arc::new(StreamJsonSink),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

const SUMMARY_KEYS: &[&str] = &[
    "file_path",
    "path",
    "command",
    "pattern",
    "patterns",
    "query",
    "url",
    "name",
    "directory",
    "glob",
    "regex",
];

fn format_tool_input(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        for &key in SUMMARY_KEYS {
            if let Some(val) = obj.get(key) {
                if let Some(s) = val.as_str() {
                    return truncate(s, 100);
                }
                if let Some(arr) = val.as_array() {
                    let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                    if !parts.is_empty() {
                        return truncate(&parts.join(", "), 100);
                    }
                }
            }
        }
    }
    let s = input.to_string();
    truncate(&s, 100)
}

#[async_trait]
impl EventSink for TextSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        match &event.kind {
            RunEventKind::AssistantMessage => {
                if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                    for block in payload.content {
                        match block {
                            AssistantBlock::Text { text } => {
                                print!("{text}");
                            }
                            AssistantBlock::ToolUse { name, input, .. } => {
                                let detail = format_tool_input(&input);
                                eprintln!("[call: {name}] {detail}");
                            }
                            AssistantBlock::Thinking { .. } => {}
                        }
                    }
                }
            }
            RunEventKind::ToolResult => {
                if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                    if payload.is_error {
                        eprintln!("[error: {}] {}", payload.tool_name, payload.content);
                    } else if !payload.content.is_empty() {
                        eprintln!(
                            "[done: {}] {}",
                            payload.tool_name,
                            truncate(&payload.content, 120)
                        );
                    }
                }
            }
            RunEventKind::Error => {
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    eprintln!("error: {}", payload.message);
                }
            }
            RunEventKind::Progress => {
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    eprintln!("[{}]", payload.message);
                }
            }
            RunEventKind::RunFinished => {
                println!();
            }
            _ => {}
        }
        Ok(())
    }
}

#[async_trait]
impl EventSink for StreamJsonSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        if let Ok(json) = serde_json::to_string(event.as_ref()) {
            println!("{json}");
        }
        Ok(())
    }
}
