use tokio_util::sync::CancellationToken;

use crate::conf::Config;
use crate::error::Result;

pub async fn start(conf: Config) -> Result<()> {
    let llm = conf.active_llm().ok();
    tracing::info!(
        id = ?conf.id,
        env_file = %conf.env_file_path.display(),
        provider = %llm.as_ref().map(|l| l.provider.as_str()).unwrap_or("unconfigured"),
        model = %llm.as_ref().map(|l| l.model.as_str()).unwrap_or("unconfigured"),
        storage = ?conf.storage.backend,
        storage_root = %conf.storage.fs.root_dir.display(),
        skills_dirs = ?conf.skills_dirs,
        sandbox = conf.sandbox.enabled,
        "server starting"
    );

    let agent = crate::bootstrap::build_agent(&conf).await?;
    let cancel = CancellationToken::new();

    // Long-lived channels (feishu, telegram, ...)
    let channel_handles = super::registry::spawn_all(&conf.channels, agent.clone(), cancel.clone());

    let channels = super::channel_tasks::ChannelTasks::new(cancel, channel_handles);
    print_banner(&conf, !channels.is_empty());

    // Always stop channel tasks, including when HTTP binding/startup fails.
    let result = super::channels::http::Server::new(agent, conf.clone())
        .start(conf.server.host.clone(), conf.server.port)
        .await;
    channels.shutdown(std::time::Duration::from_secs(5)).await;
    result
}

fn print_banner(conf: &Config, has_channels: bool) {
    let llm = conf.active_llm().ok();
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
    eprintln!(
        "  provider: {}",
        llm.as_ref()
            .map(|l| l.provider.as_str())
            .unwrap_or("unconfigured")
    );
    eprintln!(
        "  model:    {}",
        llm.as_ref()
            .map(|l| l.model.as_str())
            .unwrap_or("unconfigured")
    );
    if let Some(base_url) = llm
        .as_ref()
        .map(|l| l.base_url.as_str())
        .filter(|s| !s.is_empty())
    {
        eprintln!("  base_url: {base_url}");
    }
    eprintln!("  storage:  {storage_backend} ({storage_target})");
    if has_channels {
        let mut names = Vec::new();
        if conf.channels.feishu.is_some() {
            names.push("feishu");
        }
        eprintln!("  channels: {}", names.join(", "));
    }
    eprintln!("  ───────────────────────────────────");
    eprintln!();
}
