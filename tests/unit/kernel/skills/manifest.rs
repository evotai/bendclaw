use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillParameter;
use bendclaw::kernel::skills::skill::SkillRequirements;

#[test]
fn test_default_timeout_is_30() -> Result<()> {
    let json =
        r#"{"name":"test","description":"d","version":"1.0","executable":false,"content":""}"#;
    let s: Skill = serde_json::from_str(json)?;
    assert_eq!(s.timeout, 30);
    Ok(())
}

#[test]
fn test_serde_roundtrip() -> Result<()> {
    let skill = Skill {
        name: "grep".into(),
        description: "Search files".into(),
        version: "1.0.0".into(),
        scope: Default::default(),
        source: Default::default(),
        agent_id: None,
        user_id: None,
        timeout: 60,
        executable: true,
        parameters: vec![SkillParameter {
            name: "pattern".into(),
            description: "regex pattern".into(),
            param_type: "string".into(),
            required: true,
            default: None,
        }],
        content: String::new(),
        files: Vec::new(),
        requires: None,
    };
    let json = serde_json::to_string(&skill)?;
    let decoded: Skill = serde_json::from_str(&json)?;
    assert_eq!(decoded.name, "grep");
    assert_eq!(decoded.parameters.len(), 1);
    assert!(decoded.parameters[0].required);
    assert_eq!(decoded.timeout, 60);
    Ok(())
}

#[test]
fn test_parameter_defaults() -> Result<()> {
    let json = r#"{"name":"x","description":"d","type":"string"}"#;
    let p: SkillParameter = serde_json::from_str(json)?;
    assert!(!p.required);
    assert!(p.default.is_none());
    Ok(())
}

#[test]
fn test_fields_default() -> Result<()> {
    let json = r#"{"name":"t","description":"d","version":"1.0","executable":false,"content":""}"#;
    let s: Skill = serde_json::from_str(json)?;
    assert!(s.parameters.is_empty());
    assert!(s.files.is_empty());
    assert!(!s.executable);
    assert!(s.requires.is_none());
    Ok(())
}

#[test]
fn test_requires_serde_roundtrip() -> Result<()> {
    let skill = Skill {
        name: "cloud-sql".into(),
        description: "Run SQL".into(),
        version: "1.0.0".into(),
        scope: Default::default(),
        source: Default::default(),
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: true,
        parameters: vec![],
        content: String::new(),
        files: Vec::new(),
        requires: Some(SkillRequirements {
            bins: vec!["bendsql".into(), "jq".into()],
            env: vec!["DATABEND_DSN".into()],
        }),
    };
    let json = serde_json::to_string(&skill)?;
    let decoded: Skill = serde_json::from_str(&json)?;
    let req = decoded
        .requires
        .context("requires must survive roundtrip")?;
    assert_eq!(req.bins, vec!["bendsql", "jq"]);
    assert_eq!(req.env, vec!["DATABEND_DSN"]);
    Ok(())
}

#[test]
fn test_requires_empty_fields() -> Result<()> {
    let req = SkillRequirements {
        bins: vec![],
        env: vec![],
    };
    let json = serde_json::to_string(&req)?;
    let decoded: SkillRequirements = serde_json::from_str(&json)?;
    assert!(decoded.bins.is_empty());
    assert!(decoded.env.is_empty());
    Ok(())
}

#[test]
fn test_requires_default_from_json() -> Result<()> {
    let json = r#"{}"#;
    let req: SkillRequirements = serde_json::from_str(json)?;
    assert!(req.bins.is_empty());
    assert!(req.env.is_empty());
    Ok(())
}

#[test]
fn test_requires_partial_fields() -> Result<()> {
    let json = r#"{"bins":["curl"]}"#;
    let req: SkillRequirements = serde_json::from_str(json)?;
    assert_eq!(req.bins, vec!["curl"]);
    assert!(req.env.is_empty());
    Ok(())
}
