//! Starting the broker, and stopping it when asked.

use std::sync::Arc;

use anyhow::{Context, Result};
use kafka_broker::Broker;
use kafka_server::Server;
use kafka_storage::fs::RealFs;
use tracing_subscriber::EnvFilter;

use crate::cli::{Args, LogFormat, Settings};

/// Sets up logging to stderr.
pub(crate) fn logging(args: &Args) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr);
    match args.log_format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().init(),
    }
}

/// Opens the broker, serves until told to stop, then stops cleanly: the server first, so nothing new arrives, then
/// the broker, which flushes every partition and records that it did.
pub(crate) async fn serve(settings: Settings) -> Result<()> {
    let Settings {
        broker,
        server,
        allow_empty_users,
        ..
    } = settings;
    let broker = Arc::new(Broker::open(RealFs, broker).context("opening the broker's data")?);
    // The users stored over the API are loaded now, so only now is it known whether anybody can log in.
    if let Some(sasl) = &server.sasl {
        sasl.ensure_users(allow_empty_users)
            .map_err(|_| anyhow::anyhow!(crate::cli::NO_USERS))?;
        tracing::info!(
            configured = sasl.user_count(),
            stored = sasl.users().stored().user_count(),
            "SASL users"
        );
    }
    // A transaction whose decision was stored is carried out before anything new is taken.
    broker.recover_transactions().await;
    let running = Server::bind(server.clone(), Arc::clone(&broker))
        .await
        .with_context(|| format!("listening on {}", server.listen))?;
    tracing::info!(addr = %running.local_addr(), "listening");
    let _sweeper = broker.start_offsets_sweeper();
    let _transactions = broker.start_transactions();
    let _logs = broker.start_log_sweeper();
    let _cleaner = broker.start_log_cleaner();
    let _control = crate::control::serve(&broker);
    stopped().await;
    tracing::info!("stopping");
    running.shutdown().await;
    broker.shutdown().await.context("stopping the broker")?;
    tracing::info!("stopped");
    Ok(())
}

/// Resolves when the process is asked to stop (SIGINT or SIGTERM).
async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
