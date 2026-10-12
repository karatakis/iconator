mod api;
mod config;
mod error;
mod pages;

use anyhow::Context;
use config::Config;
use tokio::net::TcpListener;

fn load_dotenv() {
    // `.env.local` (personal overrides) first; dotenvy never overwrites
    // already-set variables, so it wins over `.env`. Missing files are fine.
    let _ = dotenvy::from_filename(".env.local");
    let _ = dotenvy::from_filename(".env");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_dotenv();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::from_env();
    let addr = format!("{}:{}", config.host, config.port);

    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    log::info!("listening on {}", listener.local_addr()?);

    let app = api::build_router();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    log::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            log::error!("failed to listen for Ctrl-C");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => log::error!("failed to listen for SIGTERM: {err}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => log::info!("Ctrl-C received, shutting down gracefully"),
        _ = terminate => log::info!("SIGTERM received, shutting down gracefully"),
    }
}
