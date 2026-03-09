use bendclaw::kernel::skills::catalog::scope_priority;
use bendclaw::kernel::skills::catalog::should_replace;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

fn skill(name: &str, scope: SkillScope, agent_id: Option<&str>, user_id: Option<&str>) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "d".to_string(),
        scope,
        source: SkillSource::Agent,
        agent_id: agent_id.map(|s| s.to_string()),
        user_id: user_id.map(|s| s.to_string()),
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "body".to_string(),
        files: vec![],
        requires: None,
    }
}

#[test]
fn scope_priority_orders_agent_user_global() {
    assert!(scope_priority(&SkillScope::Agent) > scope_priority(&SkillScope::User));
    assert!(scope_priority(&SkillScope::User) > scope_priority(&SkillScope::Global));
}

#[test]
fn should_replace_prefers_higher_scope() {
    let existing = skill("n", SkillScope::Global, None, None);
    let candidate = skill("n", SkillScope::Agent, Some("a1"), Some("u1"));
    assert!(should_replace(&existing, &candidate));
    assert!(!should_replace(&candidate, &existing));
}

#[test]
fn should_replace_same_scope_uses_stable_tie_break() {
    let existing = skill("n", SkillScope::User, None, Some("u1"));
    let candidate = skill("n", SkillScope::User, None, Some("u2"));
    assert!(should_replace(&existing, &candidate));
}

#[test]
fn should_not_replace_when_candidate_key_is_lower() {
    let existing = skill("z", SkillScope::Agent, Some("b"), Some("u2"));
    let candidate = skill("a", SkillScope::Agent, Some("a"), Some("u1"));
    assert!(!should_replace(&existing, &candidate));
}
