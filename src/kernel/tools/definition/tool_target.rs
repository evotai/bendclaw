//! Dispatch target for a tool call — determines how execution is routed.
//!
//! Lives in the bindings map, separate from `ToolDefinition` metadata.

use std::fmt;
use std::sync::Arc;

use crate::kernel::tools::tool_contract::Tool;

/// How a tool call should be dispatched at runtime.
#[derive(Clone)]
pub enum ToolTarget {
    /// In-process builtin tool — dispatch via `Tool::execute_with_context`.
    Builtin(Arc<dyn Tool>),
    /// Skill script — dispatch via `SkillExecutor`.
    Skill,
}

impl ToolTarget {
    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin(_))
    }

    pub fn is_skill(&self) -> bool {
        matches!(self, Self::Skill)
    }

    pub fn as_builtin(&self) -> Option<&Arc<dyn Tool>> {
        match self {
            Self::Builtin(tool) => Some(tool),
            Self::Skill => None,
        }
    }
}

impl fmt::Debug for ToolTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(tool) => write!(f, "Builtin({})", tool.name()),
            Self::Skill => write!(f, "Skill"),
        }
    }
}
