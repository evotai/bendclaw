use std::future::Future;
use std::future::IntoFuture;

use anyhow::Context;
use tokio_util::sync::CancellationToken;

use crate::observability::log::slog;

enum ServerExit {
    Api(std::io::Result<()>),
    Admin(std::io::Result<()>),
    Signal,
}

pub async fn supervise_servers<Api, Admin, Signal>(
    shutdown_token: CancellationToken,
    api_server: Api,
    admin_server: Option<Admin>,
    shutdown_signal: Signal,
) -> anyhow::Result<()>
where
    Api: IntoFuture<Output = std::io::Result<()>>,
    Admin: IntoFuture<Output = std::io::Result<()>>,
    Signal: Future<Output = ()>,
{
    let has_admin = admin_server.is_some();
    slog!(info, "server", "supervision_started", has_admin,);
    let api_server = async move { ServerExit::Api(api_server.into_future().await) };
    let admin_server = async move {
        match admin_server {
            Some(server) => ServerExit::Admin(server.into_future().await),
            None => std::future::pending::<ServerExit>().await,
        }
    };

    tokio::pin!(api_server);
    tokio::pin!(admin_server);
    tokio::pin!(shutdown_signal);

    let first_exit = tokio::select! {
        exit = &mut api_server => exit,
        exit = &mut admin_server => exit,
        _ = &mut shutdown_signal => {
            slog!(info, "server", "shutdown_signal",);
            ServerExit::Signal
        }
    };

    match &first_exit {
        ServerExit::Api(result) => {
            slog!(
                info,
                "server",
                "server_exit",
                listener = "api",
                success = result.is_ok(),
            );
        }
        ServerExit::Admin(result) => {
            slog!(
                info,
                "server",
                "server_exit",
                listener = "admin",
                success = result.is_ok(),
            );
        }
        ServerExit::Signal => {
            slog!(
                info,
                "server",
                "server_exit",
                listener = "signal",
                success = true,
            );
        }
    }

    shutdown_token.cancel();

    let (api_result, admin_result) = match first_exit {
        ServerExit::Api(result) => {
            let admin_result = if has_admin {
                Some(expect_admin_exit(admin_server.await))
            } else {
                None
            };
            (result, admin_result)
        }
        ServerExit::Admin(result) => {
            let api_result = expect_api_exit(api_server.await);
            (api_result, Some(result))
        }
        ServerExit::Signal => {
            let api_result = expect_api_exit(api_server.await);
            let admin_result = if has_admin {
                Some(expect_admin_exit(admin_server.await))
            } else {
                None
            };
            (api_result, admin_result)
        }
    };

    api_result.context("api server error")?;
    if let Some(result) = admin_result {
        result.context("admin server error")?;
    }
    Ok(())
}

fn expect_api_exit(exit: ServerExit) -> std::io::Result<()> {
    match exit {
        ServerExit::Api(result) => result,
        ServerExit::Admin(_) | ServerExit::Signal => unreachable!("expected api server exit"),
    }
}

fn expect_admin_exit(exit: ServerExit) -> std::io::Result<()> {
    match exit {
        ServerExit::Admin(result) => result,
        ServerExit::Api(_) | ServerExit::Signal => unreachable!("expected admin server exit"),
    }
}
