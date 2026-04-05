use crate::conf::LlmConfig;

fn provider_kind(provider: &crate::conf::ProviderKind) -> bend_agent::ProviderKind {
    match provider {
        crate::conf::ProviderKind::Anthropic => bend_agent::ProviderKind::Anthropic,
        crate::conf::ProviderKind::OpenAi => bend_agent::ProviderKind::OpenAi,
    }
}

pub fn build_agent_options(
    llm: &LlmConfig,
    cwd: Option<String>,
    max_turns: Option<u32>,
) -> bend_agent::AgentOptions {
    bend_agent::AgentOptions {
        provider: Some(provider_kind(&llm.provider)),
        model: Some(llm.model.clone()),
        api_key: Some(llm.api_key.clone()),
        base_url: llm.base_url.clone(),
        cwd,
        max_turns,
        ..Default::default()
    }
}
