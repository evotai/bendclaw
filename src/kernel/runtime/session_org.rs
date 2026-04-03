//! SessionOrgServices — minimal org interface for session runtime.

use std::sync::Arc;

use crate::kernel::memory::MemoryService;
use crate::skills::definition::skill::Skill;

/// Minimal org services needed by session core at runtime.
/// OrgServices implements this for cloud; LocalOrgServices for local.
pub trait SessionOrgServices: Send + Sync {
    fn list_skills(&self, user_id: &str) -> Vec<Skill>;
    fn memory(&self) -> Option<Arc<MemoryService>>;
}

/// Local-only: no skills, no memory.
pub struct LocalOrgServices;

impl SessionOrgServices for LocalOrgServices {
    fn list_skills(&self, _user_id: &str) -> Vec<Skill> {
        vec![]
    }
    fn memory(&self) -> Option<Arc<MemoryService>> {
        None
    }
}
