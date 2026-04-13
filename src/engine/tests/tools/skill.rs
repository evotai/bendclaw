use std::path::PathBuf;
use std::sync::Arc;

use bendengine::tools::skill::truncate_str;
use bendengine::tools::skill::SkillSet;
use bendengine::tools::skill::SkillSpec;
use bendengine::types::AgentTool;
use bendengine::types::Retention;
use bendengine::types::ToolContext;
use bendengine::SkillTool;
use tokio_util::sync::CancellationToken;

fn spec(name: &str, description: &str, instructions: &str) -> SkillSpec {
    SkillSpec {
        name: name.into(),
        description: description.into(),
        instructions: instructions.into(),
        base_dir: PathBuf::from("/test/skills").join(name),
    }
}

// ---------------------------------------------------------------------------
// SkillSet
// ---------------------------------------------------------------------------

#[test]
fn new_deduplicates_and_sorts() {
    let skills = SkillSet::new(vec![
        spec("weather", "Weather v1.", "old"),
        spec("git", "Git ops.", "git instructions"),
        spec("weather", "Weather v2.", "new"),
    ]);
    assert_eq!(skills.len(), 2);
    assert_eq!(skills.specs()[0].name, "git");
    assert_eq!(skills.specs()[1].name, "weather");
    assert_eq!(skills.specs()[1].description, "Weather v2.");
}

#[test]
fn empty_set() {
    let skills = SkillSet::empty();
    assert!(skills.is_empty());
    assert_eq!(skills.format_for_prompt(), "");
}

#[test]
fn find_by_name() {
    let skills = SkillSet::new(vec![
        spec("weather", "Get weather.", "instructions"),
        spec("git", "Git ops.", "instructions"),
    ]);
    assert!(skills.find("weather").is_some());
    assert!(skills.find("git").is_some());
    assert!(skills.find("nonexistent").is_none());
}

#[test]
fn merge_skill_sets() {
    let mut set1 = SkillSet::new(vec![
        spec("weather", "Weather v1.", "old"),
        spec("git", "Git ops.", "git"),
    ]);
    let set2 = SkillSet::new(vec![
        spec("weather", "Weather v2.", "new"),
        spec("docker", "Docker.", "docker"),
    ]);
    set1.merge(set2);

    assert_eq!(set1.len(), 3);
    let names: Vec<&str> = set1.specs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["docker", "git", "weather"]);
    assert_eq!(set1.find("weather").unwrap().description, "Weather v2.");
}

#[test]
fn format_for_prompt_has_instruction() {
    let skills = SkillSet::new(vec![spec("weather", "Get weather.", "instructions")]);
    let prompt = skills.format_for_prompt();

    assert!(prompt.contains("MUST invoke the skill tool"));
    assert!(prompt.contains("Never mention a skill"));
}

#[test]
fn format_for_prompt_empty_when_no_skills() {
    let skills = SkillSet::empty();
    assert_eq!(skills.format_for_prompt(), "");
}

// ---------------------------------------------------------------------------
// truncate_str
// ---------------------------------------------------------------------------

#[test]
fn truncate_str_short_unchanged() {
    assert_eq!(truncate_str("hello", 10), "hello");
}

#[test]
fn truncate_str_exact_length_unchanged() {
    assert_eq!(truncate_str("hello", 5), "hello");
}

#[test]
fn truncate_str_long_gets_ellipsis() {
    assert_eq!(truncate_str("hello world", 5), "hello\u{2026}");
}

#[test]
fn truncate_str_utf8_safe() {
    // "你好" = 6 bytes, "你好世" = 9 bytes; floor_char_boundary(7) = 6
    let result = truncate_str("你好世界", 7);
    assert_eq!(result, "你好\u{2026}");
}

// ---------------------------------------------------------------------------
// SkillTool
// ---------------------------------------------------------------------------

fn make_ctx() -> ToolContext {
    ToolContext {
        tool_call_id: "test".into(),
        tool_name: "skill".into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
    }
}

#[tokio::test]
async fn execute_returns_instructions() {
    let skills = SkillSet::new(vec![spec(
        "weather",
        "Get weather.",
        "# Weather\n\nDo stuff.",
    )]);
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({ "skill_name": "weather" });
    let result = tool.execute(params, make_ctx()).await.unwrap();

    let text = match &result.content[0] {
        bendengine::Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.starts_with("Activated skill: weather"));
    assert!(text.contains("must be resolved against:"));
    assert!(text.contains("# Weather"));
    assert!(text.contains("Do stuff."));
    assert_eq!(result.retention, Retention::CurrentRun);
}

#[tokio::test]
async fn execute_unknown_skill_returns_error() {
    let skills = SkillSet::new(vec![spec("weather", "Get weather.", "instructions")]);
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({ "skill_name": "nonexistent" });
    let err = tool.execute(params, make_ctx()).await.unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("Unknown skill: nonexistent"));
    assert!(msg.contains("weather"));
}

#[tokio::test]
async fn execute_missing_param_returns_error() {
    let skills = SkillSet::empty();
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({});
    let err = tool.execute(params, make_ctx()).await.unwrap_err();
    assert!(err.to_string().contains("Missing"));
}

#[test]
fn preview_command_shows_skill_name() {
    let skills = SkillSet::new(vec![spec("weather", "Get weather.", "instructions")]);
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({ "skill_name": "weather" });
    assert_eq!(
        tool.preview_command(&params),
        Some("loading skill: weather (/test/skills/weather)".into())
    );
}

#[tokio::test]
async fn execute_strips_leading_slash() {
    let skills = SkillSet::new(vec![spec("weather", "Get weather.", "instructions")]);
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({ "skill_name": "/weather" });
    let result = tool.execute(params, make_ctx()).await.unwrap();

    let text = match &result.content[0] {
        bendengine::Content::Text { text } => text,
        _ => panic!("expected text content"),
    };
    assert!(text.starts_with("Activated skill: weather"));
}

#[test]
fn preview_command_none_when_missing_param() {
    let skills = SkillSet::empty();
    let tool = SkillTool::new(Arc::new(skills));

    let params = serde_json::json!({});
    assert_eq!(tool.preview_command(&params), None);
}
