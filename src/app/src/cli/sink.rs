use std::sync::Arc;

use async_trait::async_trait;

use super::format::format_tool_input;
use super::format::truncate;
use crate::cli::app::EventSink;
use crate::cli::args::OutputFormat;
use crate::error::Result;
use crate::protocol::RunEvent;
use crate::protocol::RunEventPayload;

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
        match &event.payload {
            RunEventPayload::AssistantCompleted { content, .. } => {
                for block in content {
                    match block {
                        crate::protocol::AssistantBlock::Text { .. } => {}
                        crate::protocol::AssistantBlock::ToolCall { name, input, .. } => {
                            let detail = format_tool_input(input);
                            eprintln!("[call: {name}] {detail}");
                        }
                        crate::protocol::AssistantBlock::Thinking { .. } => {}
                    }
                }
            }
            RunEventPayload::ToolFinished {
                tool_name,
                content,
                is_error,
                ..
            } => {
                if *is_error {
                    eprintln!("[error: {tool_name}] {content}");
                } else if !content.is_empty() {
                    eprintln!("[done: {tool_name}] {}", truncate(content, 120));
                }
            }
            RunEventPayload::AssistantDelta {
                delta: Some(delta), ..
            } => {
                print!("{delta}");
            }
            RunEventPayload::Error { message } => {
                eprintln!("error: {message}");
            }
            RunEventPayload::ToolProgress { text, .. } => {
                eprintln!("[{text}]");
            }
            RunEventPayload::RunFinished { .. } => {
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
