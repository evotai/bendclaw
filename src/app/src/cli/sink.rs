use std::sync::Arc;

use async_trait::async_trait;

use super::format::format_tool_input;
use super::format::truncate;
use crate::cli::args::OutputFormat;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
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

#[async_trait]
impl EventSink for TextSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        match &event.kind {
            RunEventKind::AssistantCompleted => {
                if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                    for block in payload.content {
                        match block {
                            AssistantBlock::Text { .. } => {}
                            AssistantBlock::ToolCall { name, input, .. } => {
                                let detail = format_tool_input(&input);
                                eprintln!("[call: {name}] {detail}");
                            }
                            AssistantBlock::Thinking { .. } => {}
                        }
                    }
                }
            }
            RunEventKind::ToolFinished => {
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
            RunEventKind::AssistantDelta => {
                if let Some(delta) = event.payload.get("delta").and_then(|v| v.as_str()) {
                    print!("{delta}");
                }
            }
            RunEventKind::Error => {
                if let Some(message) = event.payload.get("message").and_then(|v| v.as_str()) {
                    eprintln!("error: {message}");
                }
            }
            RunEventKind::ToolProgress => {
                if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                    eprintln!("[{text}]");
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
