use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::agent::prompt::SystemPrompt;
use crate::agent::Agent;
use crate::conf::Config;
use crate::error::EvotError;
use crate::error::Result;

pub async fn start(conf: Config) -> Result<()> {
    let agent = build_agent(&conf)?;
    let cancel = CancellationToken::new();

    // Long-lived channels (feishu, telegram, ...)
    let channel_handles = super::registry::spawn_all(&conf.channels, agent.clone(), cancel.clone());

    print_banner(&conf, &channel_handles);

    // HTTP channel (blocking)
    super::channels::http::Server::new(agent)
        .start(conf.server.host.clone(), conf.server.port)
        .await?;

    // Shutdown
    cancel.cancel();
    for h in channel_handles {
        let _ = h.await;
    }
    Ok(())
}

fn build_agent(conf: &Config) -> Result<Arc<Agent>> {
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| EvotError::Run(format!("failed to get cwd: {e}")))?;

    let system_prompt = SystemPrompt::new(&cwd)
        .with_system()
        .with_git()
        .with_tools()
        .with_project_context()
        .with_memory()
        .with_claude_memory()
        .build();

    let mut skills_dirs = Vec::new();
    if let Ok(global) = crate::conf::paths::skills_dir() {
        skills_dirs.push(global);
    }

    Ok(Agent::new(conf, &cwd)?
        .with_system_prompt(system_prompt)
        .with_skills_dirs(skills_dirs))
}

fn print_banner(conf: &Config, channel_handles: &[tokio::task::JoinHandle<()>]) {
    let llm = conf.active_llm();
    let addr = format!("{}:{}", conf.server.host, conf.server.port);
    let storage_backend = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => "fs",
        crate::conf::StorageBackend::Cloud => "cloud",
    };
    let storage_target = match conf.storage.backend {
        crate::conf::StorageBackend::Fs => conf.storage.fs.root_dir.display().to_string(),
        crate::conf::StorageBackend::Cloud => conf.storage.cloud.endpoint.clone(),
    };

    eprintln!();
    eprintln!("  evot server");
    eprintln!("  ───────────────────────────────────");
    eprintln!("  address:  http://{addr}");
    eprintln!("  provider: {}", llm.provider);
    eprintln!("  model:    {}", llm.model);
    if let Some(ref base_url) = llm.base_url {
        if !base_url.is_empty() {
            eprintln!("  base_url: {base_url}");
        }
    }
    eprintln!("  storage:  {storage_backend} ({storage_target})");
    if !channel_handles.is_empty() {
        let mut names = Vec::new();
        if conf.channels.feishu.is_some() {
            names.push("feishu");
        }
        eprintln!("  channels: {}", names.join(", "));
    }
    eprintln!("  ───────────────────────────────────");
    eprintln!();
}
