use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::Json;
use axum::Router;
use bendclaw::client::DirectiveClient;
use bendclaw::directive::DirectiveService;
use parking_lot::RwLock;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct TestState {
    prompt: Arc<RwLock<String>>,
}

async fn directive(State(state): State<TestState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "prompt": state.prompt.read().clone(),
    }))
}

async fn spawn_directive_server(
    prompt: Arc<RwLock<String>>,
) -> anyhow::Result<(String, oneshot::Sender<()>, tokio::task::JoinHandle<()>)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let router = Router::new()
        .route("/v1/directive", get(directive))
        .with_state(TestState { prompt });

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let shutdown = async {
            let _ = shutdown_rx.await;
        };
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await;
    });

    Ok((format!("http://{addr}"), shutdown_tx, handle))
}

#[tokio::test]
async fn refresh_updates_cached_prompt() -> anyhow::Result<()> {
    let prompt = Arc::new(RwLock::new("Initial".to_string()));
    let (base_url, shutdown_tx, handle) = spawn_directive_server(prompt.clone()).await?;
    let client = Arc::new(DirectiveClient::new(base_url, "test-token")?);
    let service = DirectiveService::new(client, Duration::from_secs(60));

    assert_eq!(service.cached_prompt(), None);
    assert_eq!(service.refresh().await?, Some("Initial".to_string()));
    assert_eq!(service.cached_prompt(), Some("Initial".to_string()));

    *prompt.write() = "Updated".to_string();
    assert_eq!(service.refresh().await?, Some("Updated".to_string()));
    assert_eq!(service.cached_prompt(), Some("Updated".to_string()));

    let _ = shutdown_tx.send(());
    let _ = handle.await;
    Ok(())
}

#[tokio::test]
async fn refresh_loop_keeps_cache_hot() -> anyhow::Result<()> {
    let prompt = Arc::new(RwLock::new("v1".to_string()));
    let (base_url, shutdown_tx, handle) = spawn_directive_server(prompt.clone()).await?;
    let client = Arc::new(DirectiveClient::new(base_url, "test-token")?);
    let service = Arc::new(DirectiveService::new(client, Duration::from_millis(25)));

    service.refresh().await?;
    assert_eq!(service.cached_prompt(), Some("v1".to_string()));

    let cancel = CancellationToken::new();
    let refresh_handle = service.spawn_refresh_loop(cancel.clone());

    *prompt.write() = "v2".to_string();
    tokio::time::sleep(Duration::from_millis(80)).await;

    assert_eq!(service.cached_prompt(), Some("v2".to_string()));

    cancel.cancel();
    let _ = refresh_handle.await;
    let _ = shutdown_tx.send(());
    let _ = handle.await;
    Ok(())
}
