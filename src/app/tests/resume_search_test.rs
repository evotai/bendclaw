use evot::agent::resume_search::build_rank_prompt;
use evot::agent::resume_search::format_results;
use evot::agent::resume_search::literal_results;
use evot::agent::Agent;
use evot::agent::QueryRequest;
use evot::agent::SubmitOutcome;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::search::SessionWithText;
use evot::types::SessionMeta;
use tempfile::TempDir;

fn session(id: &str, title: &str, text: &str) -> SessionWithText {
    let mut meta = SessionMeta::new(id.to_string(), "/tmp/proj".to_string(), "m".to_string());
    meta.title = Some(title.to_string());
    SessionWithText {
        session: meta,
        search_text: text.to_string(),
        user_prompts: vec![],
    }
}

#[test]
fn rank_prompt_embeds_query_and_session_ids() {
    let sessions = vec![
        session("s1", "tailscale migration", "moved nodes between accounts"),
        session("s2", "spill oom", "databend spill buffer"),
    ];
    let prompt = build_rank_prompt("vpn account handover", &sessions);
    assert!(prompt.starts_with("Query: vpn account handover"));
    assert!(prompt.contains("id: s1"));
    assert!(prompt.contains("id: s2"));
    assert!(prompt.contains("moved nodes between accounts"));
}

#[test]
fn rank_prompt_truncates_long_transcripts() {
    let long_text = "x".repeat(10_000);
    let sessions = vec![session("s1", "big", &long_text)];
    let prompt = build_rank_prompt("q", &sessions);
    assert!(prompt.len() < 3_000);
}

#[test]
fn literal_results_return_exact_matches_without_llm_ranking() {
    let sessions = vec![
        session(
            "s1",
            "flight form",
            "assistant thinking: incident NEBULA-4729",
        ),
        session("s2", "spill oom", "databend spill buffer"),
    ];
    let out = literal_results("NEBULA-4729", &sessions).ok_or("missing literal result");
    assert!(out.is_ok());
    let out = out.unwrap_or_default();
    assert!(out.contains("- s1 — flight form — exact text match"));
    assert!(!out.contains("s2"));
    assert!(literal_results("'NEBULA-4729'", &sessions).is_some());
    assert!(literal_results("\"NEBULA-4729\"", &sessions).is_some());
    assert!(literal_results("missing", &sessions).is_none());
}

#[test]
fn format_results_lists_matches_and_drops_hallucinated_ids() {
    let sessions = vec![
        session("s1", "tailscale migration", ""),
        session("s2", "spill oom", ""),
    ];
    let response = "s1 | moved tailscale nodes\nbogus-id | should vanish\ns2 | oom fix";
    let out = format_results("vpn", response, &sessions);
    assert!(out.contains("- s1 — tailscale migration — moved tailscale nodes"));
    assert!(out.contains("- s2 — spill oom — oom fix"));
    assert!(!out.contains("bogus-id"));
    assert!(out.contains("Resume with /resume <id>."));
}

#[test]
fn format_results_none_and_garbage_report_no_matches() {
    let sessions = vec![session("s1", "t", "")];
    assert!(format_results("q", "NONE", &sessions).starts_with("No sessions relevant"));
    assert!(format_results("q", "free-form chatter", &sessions).starts_with("No sessions relevant"));
}

/// `/_rsearch` is session-independent: submitting it without a session id
/// (the fresh-CLI `/resume <query>` path) must not persist an empty session.
/// With an empty vault it answers before any LLM call, so no network needed.
#[tokio::test]
async fn rsearch_submit_does_not_persist_a_session(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let mut config = evot::conf::Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "http://localhost".into(),
        models: vec!["test-model".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();

    let agent = Agent::new(&config, "/work")?;
    let outcome = agent
        .submit(QueryRequest::text("/_rsearch tailscale migration"))
        .await?;
    match outcome {
        SubmitOutcome::Command(msg) => assert_eq!(msg, "No sessions to search."),
        SubmitOutcome::Run(_) => return Err("expected command outcome, got run".into()),
    }

    let sessions = agent.list_sessions(0).await?;
    assert!(
        sessions.is_empty(),
        "resume search must not create sessions, found: {sessions:?}"
    );
    Ok(())
}
