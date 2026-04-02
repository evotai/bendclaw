use std::path::PathBuf;
use std::sync::Arc;

/// Build a test `SkillCatalog` backed by a temp directory (no DB needed for hub-only tests).
#[allow(dead_code)]
pub fn test_skill_projector(
    workspace_root: PathBuf,
) -> Arc<bendclaw::kernel::skills::sync::SkillCatalog> {
    Arc::new(bendclaw::kernel::skills::sync::SkillCatalog::new(
        workspace_root,
        Arc::new(bendclaw_test_harness::mocks::skill::NoopSkillStore),
        Arc::new(bendclaw_test_harness::mocks::skill::NoopSubscriptionStore),
        None,
    ))
}
