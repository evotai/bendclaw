use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillParameter;
use bendclaw_test_harness::mocks::skill::MockSkillCatalog;

fn demo_skill(name: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "demo".to_string(),
        scope: Default::default(),
        source: Default::default(),
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: true,
        parameters: vec![SkillParameter {
            name: "input".to_string(),
            description: "input".to_string(),
            param_type: "string".to_string(),
            required: true,
            default: None,
        }],
        content: "# skill doc".to_string(),
        files: vec![SkillFile {
            path: "scripts/run.py".to_string(),
            body: "print('ok')".to_string(),
        }],
        requires: None,
    }
}

#[test]
fn mock_skill_catalog_insert_get_and_evict() {
    let catalog = MockSkillCatalog::new();
    let skill = demo_skill("alpha");

    catalog.insert(&skill);
    assert!(catalog.contains("alpha"));
    assert!(catalog.get("alpha").is_some());

    catalog.evict("alpha");
    assert!(!catalog.contains("alpha"));
    assert!(catalog.get("alpha").is_none());
}

#[test]
fn mock_skill_catalog_resolve_and_script_path() {
    let catalog = MockSkillCatalog::new();
    let skill = demo_skill("run-task");
    catalog.insert(&skill);

    let resolved = catalog.resolve("run-task");
    assert!(resolved.is_some());
    assert_eq!(
        catalog.script_path("run-task").as_deref(),
        Some("scripts/run.py")
    );
}
