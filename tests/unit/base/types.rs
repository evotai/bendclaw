use anyhow::bail;
use anyhow::Result;
use bendclaw::kernel::new_id;
use bendclaw::kernel::sanitize_agent_id;
use bendclaw::kernel::Content;
use bendclaw::kernel::ErrorSource;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;
use bendclaw::kernel::Role;
use bendclaw::kernel::ToolCall;

// ── Role ──

#[test]
fn role_display_all_variants() {
    assert_eq!(Role::System.to_string(), "system");
    assert_eq!(Role::User.to_string(), "user");
    assert_eq!(Role::Assistant.to_string(), "assistant");
    assert_eq!(Role::Tool.to_string(), "tool");
}

#[test]
fn role_serde_roundtrip() -> Result<()> {
    for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
        let json = serde_json::to_string(&role)?;
        let back: Role = serde_json::from_str(&json)?;
        assert_eq!(back, role);
    }
    Ok(())
}

#[test]
fn role_serde_rename_lowercase() -> Result<()> {
    let json = serde_json::to_string(&Role::Assistant)?;
    assert_eq!(json, "\"assistant\"");
    Ok(())
}

// ── Content ──

#[test]
fn text_constructor_and_as_text() {
    let c = Content::text("hello");
    assert_eq!(c.as_text(), "hello");
}

#[test]
fn image_constructor() -> Result<()> {
    let c = Content::image("base64data", "image/png");
    match &c {
        Content::Image { data, mime_type } => {
            assert_eq!(data, "base64data");
            assert_eq!(mime_type, "image/png");
        }
        _ => bail!("expected Image variant"),
    }
    Ok(())
}

#[test]
fn image_as_text_is_empty() {
    let c = Content::image("data", "image/jpeg");
    assert_eq!(c.as_text(), "");
}

#[test]
fn content_serde_roundtrip_text() -> Result<()> {
    let c = Content::text("hello world");
    let json = serde_json::to_string(&c)?;
    let back: Content = serde_json::from_str(&json)?;
    assert_eq!(back.as_text(), "hello world");
    Ok(())
}

#[test]
fn content_serde_roundtrip_image() -> Result<()> {
    let c = Content::image("abc", "image/png");
    let json = serde_json::to_string(&c)?;
    let back: Content = serde_json::from_str(&json)?;
    assert_eq!(back.as_text(), "");
    assert!(json.contains("\"type\":\"image\""));
    Ok(())
}

#[test]
fn content_serde_tagged_type_field() -> Result<()> {
    let json = serde_json::to_string(&Content::text("x"))?;
    assert!(json.contains("\"type\":\"text\""));
    Ok(())
}

// ── ToolCall ──

#[test]
fn tool_call_serde_roundtrip() -> Result<()> {
    let tc = ToolCall {
        id: "tc1".into(),
        name: "shell".into(),
        arguments: r#"{"cmd":"ls"}"#.into(),
    };
    let json = serde_json::to_string(&tc)?;
    let back: ToolCall = serde_json::from_str(&json)?;
    assert_eq!(back.id, "tc1");
    assert_eq!(back.name, "shell");
    assert_eq!(back.arguments, r#"{"cmd":"ls"}"#);
    Ok(())
}

#[test]
fn tool_call_clone() {
    let tc = ToolCall {
        id: "tc1".into(),
        name: "databend".into(),
        arguments: "{}".into(),
    };
    let cloned = tc.clone();
    assert_eq!(cloned.id, tc.id);
    assert_eq!(cloned.name, tc.name);
}
// ── ErrorSource ──

#[test]
fn error_source_display_llm() {
    assert_eq!(ErrorSource::Llm.to_string(), "llm");
}

#[test]
fn error_source_display_tool() {
    assert_eq!(
        ErrorSource::Tool("databend".into()).to_string(),
        "tool:databend"
    );
}

#[test]
fn error_source_display_skill() {
    assert_eq!(
        ErrorSource::Skill("python-runner".into()).to_string(),
        "skill:python-runner"
    );
}

#[test]
fn error_source_display_sandbox() {
    assert_eq!(ErrorSource::Sandbox.to_string(), "sandbox");
}

#[test]
fn error_source_display_internal() {
    assert_eq!(ErrorSource::Internal.to_string(), "internal");
}

#[test]
fn error_source_serde_roundtrip_llm() -> Result<()> {
    let src = ErrorSource::Llm;
    let json = serde_json::to_string(&src)?;
    assert_eq!(json, "\"llm\"");
    let back: ErrorSource = serde_json::from_str(&json)?;
    assert_eq!(back, src);
    Ok(())
}

#[test]
fn error_source_serde_roundtrip_tool() -> Result<()> {
    let src = ErrorSource::Tool("shell".into());
    let json = serde_json::to_string(&src)?;
    assert_eq!(json, "\"tool:shell\"");
    let back: ErrorSource = serde_json::from_str(&json)?;
    assert_eq!(back, src);
    Ok(())
}

#[test]
fn error_source_serde_roundtrip_skill() -> Result<()> {
    let src = ErrorSource::Skill("my-skill".into());
    let json = serde_json::to_string(&src)?;
    let back: ErrorSource = serde_json::from_str(&json)?;
    assert_eq!(back, src);
    Ok(())
}

#[test]
fn error_source_serde_roundtrip_sandbox() -> Result<()> {
    let src = ErrorSource::Sandbox;
    let json = serde_json::to_string(&src)?;
    let back: ErrorSource = serde_json::from_str(&json)?;
    assert_eq!(back, src);
    Ok(())
}

#[test]
fn error_source_serde_roundtrip_internal() -> Result<()> {
    let src = ErrorSource::Internal;
    let json = serde_json::to_string(&src)?;
    let back: ErrorSource = serde_json::from_str(&json)?;
    assert_eq!(back, src);
    Ok(())
}

#[test]
fn error_source_deserialize_unknown_falls_back_to_internal() -> Result<()> {
    let back: ErrorSource = serde_json::from_str("\"something_unknown\"")?;
    assert_eq!(back, ErrorSource::Internal);
    Ok(())
}

// ── Impact ──

#[test]
fn impact_display_all_variants() {
    assert_eq!(Impact::Low.to_string(), "low");
    assert_eq!(Impact::Medium.to_string(), "medium");
    assert_eq!(Impact::High.to_string(), "high");
}

#[test]
fn impact_serde_roundtrip() -> Result<()> {
    for impact in [Impact::Low, Impact::Medium, Impact::High] {
        let json = serde_json::to_string(&impact)?;
        let back: Impact = serde_json::from_str(&json)?;
        assert_eq!(back, impact);
    }
    Ok(())
}

#[test]
fn impact_clone_and_eq() {
    let a = Impact::High;
    let b = a.clone();
    assert_eq!(a, b);
}
// ── OpType ──

#[test]
fn op_type_display_all_variants() {
    assert_eq!(OpType::Reasoning.to_string(), "REASONING");
    assert_eq!(OpType::Execute.to_string(), "EXECUTE");
    assert_eq!(OpType::Edit.to_string(), "EDIT");
    assert_eq!(OpType::FileRead.to_string(), "FILE_READ");
    assert_eq!(OpType::FileWrite.to_string(), "FILE_WRITE");
    assert_eq!(OpType::MemoryWrite.to_string(), "MEMORY_WRITE");
    assert_eq!(OpType::MemoryRead.to_string(), "MEMORY_READ");
    assert_eq!(OpType::MemorySearch.to_string(), "MEMORY_SEARCH");
    assert_eq!(OpType::MemoryDelete.to_string(), "MEMORY_DELETE");
    assert_eq!(OpType::MemoryList.to_string(), "MEMORY_LIST");
    assert_eq!(OpType::SkillRun.to_string(), "SKILL_RUN");
    assert_eq!(OpType::Compaction.to_string(), "COMPACTION");
    assert_eq!(OpType::Checkpoint.to_string(), "CHECKPOINT");
    assert_eq!(OpType::Databend.to_string(), "DATABEND");
}

#[test]
fn op_type_serde_roundtrip() -> Result<()> {
    let op = OpType::Execute;
    let json = serde_json::to_string(&op)?;
    let back: OpType = serde_json::from_str(&json)?;
    assert_eq!(back, op);
    Ok(())
}

#[test]
fn op_type_clone_and_eq() {
    let a = OpType::SkillRun;
    let b = a.clone();
    assert_eq!(a, b);
}

// ── Utils ──

#[test]
fn new_id_is_unique() {
    let a = new_id();
    let b = new_id();
    assert_ne!(a, b);
}

#[test]
fn new_id_is_lowercase() {
    let id = new_id();
    assert_eq!(id, id.to_lowercase());
}

#[test]
fn new_id_is_not_empty() {
    assert!(!new_id().is_empty());
}

#[test]
fn sanitize_simple() {
    assert_eq!(sanitize_agent_id("myAgent"), "myagent");
}

#[test]
fn sanitize_special_chars() {
    assert_eq!(sanitize_agent_id("my-agent!v2"), "my_agent_v2");
}

#[test]
fn sanitize_empty() {
    assert_eq!(sanitize_agent_id(""), "default");
}

#[test]
fn sanitize_whitespace_only() {
    assert_eq!(sanitize_agent_id("   "), "default");
}

#[test]
fn sanitize_consecutive_specials() {
    assert_eq!(sanitize_agent_id("a--b..c"), "a_b_c");
}

#[test]
fn sanitize_leading_trailing_specials() {
    assert_eq!(sanitize_agent_id("--agent--"), "agent");
}
