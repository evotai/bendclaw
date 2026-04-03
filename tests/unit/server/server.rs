use std::future::pending;
use std::future::ready;
use std::io::Error;

use bendclaw::server::server::supervise_servers;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn admin_exit_notifies_api_shutdown() {
    let shutdown_token = CancellationToken::new();
    let api_shutdown = shutdown_token.clone();
    let (api_stopped_tx, api_stopped_rx) = oneshot::channel();

    let api_server = async move {
        api_shutdown.cancelled().await;
        let _ = api_stopped_tx.send(());
        Ok(())
    };
    let admin_server = async { Err(Error::other("admin failed")) };

    let result = supervise_servers(shutdown_token, api_server, Some(admin_server), pending()).await;

    assert!(result.is_err());
    api_stopped_rx.await.expect("api shutdown observed");
}

#[tokio::test]
async fn api_exit_notifies_admin_shutdown() {
    let shutdown_token = CancellationToken::new();
    let admin_shutdown = shutdown_token.clone();
    let (admin_stopped_tx, admin_stopped_rx) = oneshot::channel();

    let api_server = async { Ok(()) };
    let admin_server = async move {
        admin_shutdown.cancelled().await;
        let _ = admin_stopped_tx.send(());
        Ok(())
    };

    supervise_servers(shutdown_token, api_server, Some(admin_server), pending())
        .await
        .expect("servers supervised");

    admin_stopped_rx.await.expect("admin shutdown observed");
}

#[tokio::test]
async fn shutdown_signal_notifies_all_servers() {
    let shutdown_token = CancellationToken::new();
    let api_shutdown = shutdown_token.clone();
    let admin_shutdown = shutdown_token.clone();
    let (api_stopped_tx, api_stopped_rx) = oneshot::channel();
    let (admin_stopped_tx, admin_stopped_rx) = oneshot::channel();

    let api_server = async move {
        api_shutdown.cancelled().await;
        let _ = api_stopped_tx.send(());
        Ok(())
    };
    let admin_server = async move {
        admin_shutdown.cancelled().await;
        let _ = admin_stopped_tx.send(());
        Ok(())
    };

    supervise_servers(shutdown_token, api_server, Some(admin_server), ready(()))
        .await
        .expect("signal handled");

    api_stopped_rx.await.expect("api shutdown observed");
    admin_stopped_rx.await.expect("admin shutdown observed");
}
