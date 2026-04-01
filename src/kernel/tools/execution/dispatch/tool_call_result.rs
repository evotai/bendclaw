use crate::kernel::OperationMeta;

/// Hard upper limit on any tool/skill output (~64K tokens).
const MAX_TOOL_OUTPUT: usize = 256_000;

/// Semantic result of a single tool/skill call.
#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Success(String, OperationMeta),
    ToolError(String, OperationMeta),
    InfraError(String, OperationMeta),
}

impl ToolCallResult {
    pub fn operation(&self) -> &OperationMeta {
        match self {
            Self::Success(_, meta) | Self::ToolError(_, meta) | Self::InfraError(_, meta) => meta,
        }
    }
}

pub(crate) fn truncate_output(text: String) -> String {
    if text.len() <= MAX_TOOL_OUTPUT {
        return text;
    }
    crate::base::truncate_with_notice(&text, MAX_TOOL_OUTPUT)
}
