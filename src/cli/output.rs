use std::sync::Arc;

use async_trait::async_trait;

use crate::cli::args::OutputFormat;
use crate::error::Result;
use crate::run::AssistantBlock;
use crate::run::AssistantPayload;
use crate::run::EventSink;
use crate::run::MessagePayload;
use crate::run::RunEvent;
use crate::run::RunEventKind;
use crate::run::ToolResultPayload;

pub struct TextSink;

pub struct StreamJsonSink;

pub fn create_sink(format: &OutputFormat) -> Box<dyn EventSink> {
    match format {
        OutputFormat::Text => Box::new(TextSink),
        OutputFormat::StreamJson => Box::new(StreamJsonSink),
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
                if let Some(payload) = event.payload_as::<AssistantPayload>() {
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
                if let Some(payload) = event.payload_as::<ToolResultPayload>() {
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
                if let Some(payload) = event.payload_as::<MessagePayload>() {
                    eprintln!("error: {}", payload.message);
                }
            }
            RunEventKind::Progress => {
                if let Some(payload) = event.payload_as::<MessagePayload>() {
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
