//! Runtime contract for recording skill usage after execution.

use crate::kernel::skills::definition::skill::SkillId;

/// Fire-and-forget usage tracking for skill execution.
pub trait UsageSink: Send + Sync + 'static {
    fn touch_used(&self, id: SkillId, agent_id: String);
}
