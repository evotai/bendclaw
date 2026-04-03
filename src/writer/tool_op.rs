use crate::writer::BackgroundWriter;

pub enum ToolWriteOp {}

pub type ToolWriter = BackgroundWriter<ToolWriteOp>;

pub fn spawn_tool_writer() -> ToolWriter {
    BackgroundWriter::spawn("tool_write", 256, |_op| async { true })
}
