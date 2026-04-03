use anyhow::bail;
use anyhow::Result;
use bendclaw::execution::Event;
use bendclaw::kernel::skills::definition::skill::Skill;
use bendclaw::kernel::workbench::sem_event::capture_capabilities;
use bendclaw::kernel::workbench::sem_event::SemEvent;
use bendclaw::llm::tool::FunctionDef;
use bendclaw::llm::tool::ToolSchema;

// ── SemEvent serde ──

#[test]
fn sem_event_capabilities_snapshot_serde_roundtrip() -> Result<()> {
    let sem = SemEvent::CapabilitiesSnapshot {
        tools: vec!["shell".into(), "grep".into()],
        skills: vec!["commit".into()],
    };
    let json = serde_json::to_string(&sem)?;
    let back: SemEvent = serde_json::from_str(&json)?;
    match back {
        SemEvent::CapabilitiesSnapshot { tools, skills } => {
            assert_eq!(tools, vec!["shell", "grep"]);
            assert_eq!(skills, vec!["commit"]);
        }
    }
    Ok(())
}

#[test]
fn sem_event_name() {
    let sem = SemEvent::CapabilitiesSnapshot {
        tools: vec![],
        skills: vec![],
    };
    assert_eq!(sem.name(), "sem.capabilities_snapshot");
}

// ── Event::Semantic serde ──

#[test]
fn event_semantic_serde_roundtrip() -> Result<()> {
    let e = Event::Semantic(SemEvent::CapabilitiesSnapshot {
        tools: vec!["shell".into()],
        skills: vec!["review".into()],
    });
    let json = serde_json::to_string(&e)?;
    let back: Event = serde_json::from_str(&json)?;
    match back {
        Event::Semantic(SemEvent::CapabilitiesSnapshot { tools, skills }) => {
            assert_eq!(tools, vec!["shell"]);
            assert_eq!(skills, vec!["review"]);
        }
        _ => bail!("expected Semantic(CapabilitiesSnapshot)"),
    }
    Ok(())
}

#[test]
fn event_semantic_name() {
    let e = Event::Semantic(SemEvent::CapabilitiesSnapshot {
        tools: vec![],
        skills: vec![],
    });
    assert_eq!(e.name(), "sem.capabilities_snapshot");
}

// ── capture_capabilities ──

fn make_skill(name: &str, user_id: &str, executable: bool) -> Skill {
    Skill {
        name: name.to_string(),
        user_id: user_id.to_string(),
        executable,
        description: "test".to_string(),
        content: String::new(),
        version: String::new(),
        scope: Default::default(),
        source: Default::default(),
        created_by: None,
        last_used_by: None,
        timeout: 30,
        parameters: vec![],
        files: vec![],
        requires: None,
        manifest: None,
    }
}

#[test]
fn capture_capabilities_filters_executable_and_formats_names() {
    let tools = vec![ToolSchema {
        schema_type: "function".into(),
        function: FunctionDef {
            name: "shell".into(),
            description: "run shell".into(),
            parameters: serde_json::json!({}),
        },
    }];
    let skills = vec![
        make_skill("commit", "user_1", false), // non-exec owned → "commit"
        make_skill("deploy", "user_1", true),  // exec → filtered out
        make_skill("review", "other_user", false), // non-exec subscribed → "other_user/review"
    ];
    let event = capture_capabilities(&tools, &skills, "user_1");
    match event {
        Event::Semantic(SemEvent::CapabilitiesSnapshot {
            tools: t,
            skills: s,
        }) => {
            assert_eq!(t, vec!["shell"]);
            assert_eq!(s, vec!["commit", "other_user/review"]);
        }
        _ => panic!("expected Semantic(CapabilitiesSnapshot)"),
    }
}
