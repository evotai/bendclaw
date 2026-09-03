use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::QueryRequest;
use evot::agent::SubmitOutcome;
use evot::agent::ToolMode;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::storage::MemoryStorage;
use evot::types::TranscriptStats;
use evot_engine::provider::MockProvider;
use tempfile::TempDir;

fn test_agent(tmp: &TempDir) -> Result<Arc<Agent>, Box<dyn std::error::Error>> {
    let mut config = Config::new(tmp.path().join("state"));
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
    Ok(Agent::new_with_provider_for_test(
        &config,
        tmp.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        MockProvider::text("ok"),
    )?)
}

async fn first_request_prompt(
    agent: &Arc<Agent>,
    request: QueryRequest,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let meta = agent.create_session("test").await?;
    let session = agent
        .load_session(&meta.session_id)
        .await?
        .ok_or("missing session")?;
    let outcome = agent.submit_to_session(request, session).await?;
    let mut run = match outcome {
        SubmitOutcome::Run(run) => run,
        SubmitOutcome::Command(message) => return Err(message.into()),
    };
    while run.next().await.is_some() {}

    let transcript = agent.load_transcript(&meta.session_id).await?;
    let started = transcript
        .iter()
        .find_map(TranscriptStats::try_from_item)
        .and_then(|stats| match stats {
            TranscriptStats::LlmCallStarted(started) => Some(started),
            _ => None,
        })
        .ok_or("missing llm call")?;
    Ok((
        started.system_prompt,
        started
            .tool_definitions
            .into_iter()
            .map(|tool| tool.name)
            .collect(),
    ))
}

#[tokio::test]
async fn selected_skills_filter_the_system_prompt() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let agent = test_agent(&tmp)?;
    agent.set_skill_names(vec!["harden".into()])?;

    let (prompt, tools) = first_request_prompt(
        &agent,
        QueryRequest::text("harden this").mode(ToolMode::Interactive),
    )
    .await?;

    assert!(prompt.contains("<available_skills>"));
    assert!(prompt.contains("<name>harden</name>"));
    assert!(!prompt.contains("<name>memory</name>"));
    assert!(!tools.iter().any(|tool| tool == "skill"));
    Ok(())
}

#[tokio::test]
async fn empty_skill_selection_omits_the_index() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let agent = test_agent(&tmp)?;
    agent.set_skill_names(Vec::new())?;

    let (prompt, tools) =
        first_request_prompt(&agent, QueryRequest::text("hello").mode(ToolMode::Headless)).await?;

    assert!(!prompt.contains("<available_skills>"));
    assert!(!tools.iter().any(|tool| tool == "skill"));
    Ok(())
}
