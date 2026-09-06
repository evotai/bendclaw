use std::sync::Arc;
use std::time::Duration;

use evot::agent::Agent;
use evot::agent::QueryRequest;
use evot::agent::RunManager;
use evot::agent::SendOutcome;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::sessions::SessionLocator;
use evot::storage::MemoryStorage;
use evot_engine::provider::ProviderError;
use evot_engine::provider::StreamConfig;
use evot_engine::provider::StreamEvent;
use evot_engine::provider::StreamOutcome;
use evot_engine::provider::StreamProvider;
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct FloodingProvider;

#[async_trait::async_trait]
impl StreamProvider for FloodingProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let _ = tx.send(StreamEvent::Start);
        for _ in 0..1024 {
            if cancel.is_cancelled() {
                break;
            }
            let _ = tx.send(StreamEvent::TextDelta {
                content_index: 0,
                delta: "fixture".into(),
            });
            tokio::task::yield_now().await;
        }
        cancel.cancelled().await;
        Err(ProviderError::Cancelled)
    }
}

#[tokio::test]
async fn unread_event_overflow_cancels_run_and_persists_cleanup_without_consumer() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "fixture".into(),
        base_url: "http://127.0.0.1:1".into(),
        models: vec!["fixture".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();
    let agent = Agent::new_with_provider_for_test(
        &config,
        dir.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        FloodingProvider,
    )?;
    let mut run = match agent.submit(QueryRequest::text("flood")).await? {
        evot::agent::SubmitOutcome::Run(run) => run,
        _ => return Err("expected run".into()),
    };
    let id = run.session_id.clone();
    // Do not poll Run::next until persistence and registry cleanup finish.
    tokio::time::timeout(Duration::from_secs(3), async {
        while agent.has_active_run(&id) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(run.handle().is_cancelled());
    assert!(!agent.sessions().transcript(&id).await?.is_empty());
    let mut kinds = Vec::new();
    let mut overflow = false;
    while let Some(event) = run.next().await {
        if let evot::agent::RunEventPayload::Error { message } = &event.payload {
            overflow |= message.contains("consumer is too slow");
        }
        kinds.push(event.kind_str().to_string());
    }
    assert!(overflow);
    assert!(kinds.len() <= 258);
    assert_eq!(kinds.last().map(String::as_str), Some("run_finished"));
    Ok(())
}

struct WaitingProvider {
    started: CancellationToken,
    cancelled: CancellationToken,
}

#[async_trait::async_trait]
impl StreamProvider for WaitingProvider {
    async fn stream(
        &self,
        _config: StreamConfig,
        _tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
        cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        self.started.cancel();
        cancel.cancelled().await;
        self.cancelled.cancel();
        Err(ProviderError::Cancelled)
    }
}

#[tokio::test]
async fn dropping_idle_run_consumer_cancels_engine_and_persists_cleanup() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "fixture".into(),
        base_url: "http://127.0.0.1:1".into(),
        models: vec!["fixture".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();
    let started = CancellationToken::new();
    let cancelled = CancellationToken::new();
    let agent = Agent::new_with_provider_for_test(
        &config,
        dir.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        WaitingProvider {
            started: started.clone(),
            cancelled: cancelled.clone(),
        },
    )?;
    let outcome = agent.submit(QueryRequest::text("wait")).await?;
    let run = match outcome {
        evot::agent::SubmitOutcome::Run(run) => run,
        _ => return Err("expected run".into()),
    };
    let id = run.session_id.clone();
    let control = run.handle();
    tokio::time::timeout(Duration::from_secs(3), started.cancelled()).await?;
    drop(run);
    tokio::time::timeout(Duration::from_secs(3), cancelled.cancelled()).await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while agent.has_active_run(&id) {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(control.is_cancelled());
    assert!(agent.sessions().find(&id).await?.is_some());
    assert!(!agent.sessions().transcript(&id).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn immediate_abort_does_not_start_a_late_provider_or_strand_registry() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "fixture".into(),
        base_url: "http://127.0.0.1:1".into(),
        models: vec!["fixture".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();
    let started = CancellationToken::new();
    let agent = Agent::new_with_provider_for_test(
        &config,
        dir.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        WaitingProvider {
            started: started.clone(),
            cancelled: CancellationToken::new(),
        },
    )?;
    let outcome = agent
        .submit(QueryRequest::text("cancel before execution"))
        .await?;
    let mut run = match outcome {
        evot::agent::SubmitOutcome::Run(run) => run,
        _ => return Err("expected run".into()),
    };
    let id = run.session_id.clone();
    run.abort();
    tokio::time::timeout(Duration::from_secs(3), async {
        while run.next().await.is_some() {}
    })
    .await?;
    assert!(!agent.has_active_run(&id));
    assert!(!started.is_cancelled());
    Ok(())
}

#[tokio::test]
async fn concurrent_channel_submissions_start_one_run_and_steer_the_other() -> TestResult {
    let dir = tempfile::tempdir()?;
    let mut config = Config::new(dir.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "fixture".into(),
        base_url: "http://127.0.0.1:1".into(),
        models: vec!["fixture".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();
    let agent = Agent::new_with_provider_for_test(
        &config,
        dir.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        WaitingProvider {
            started: CancellationToken::new(),
            cancelled: CancellationToken::new(),
        },
    )?;
    let manager = RunManager::new(agent.clone());
    let locator = SessionLocator::new("fixture", "same-conversation");
    let (first, second) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(
            manager.send(&locator, QueryRequest::text("one")),
            manager.send(&locator, QueryRequest::text("two")),
        )
    })
    .await?;
    let mut run = match (first?, second?) {
        (SendOutcome::Started(run), SendOutcome::Steered)
        | (SendOutcome::Steered, SendOutcome::Started(run)) => run,
        _ => return Err("expected one started run and one steering submission".into()),
    };
    let blocked = manager
        .send(&locator, QueryRequest::text("/compact"))
        .await?;
    assert!(
        matches!(blocked, SendOutcome::Command(message) if message.contains("Commands don't queue"))
    );
    for entry in run.handle().queued_steering() {
        assert!(!serde_json::to_string(&entry.message)?.contains("/compact"));
    }
    run.abort();
    tokio::time::timeout(Duration::from_secs(3), async {
        while run.next().await.is_some() {}
    })
    .await?;
    assert!(!agent.has_active_run(&locator.session_id()));
    Ok(())
}
