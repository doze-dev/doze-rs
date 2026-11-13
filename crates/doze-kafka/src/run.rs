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

/// The console's listener as `main` had it bound before anything was created.
#[cfg(feature = "console")]
pub(crate) type ConsoleBound = Option<crate::web::Bound>;
/// A build without the console has none to bind.
#[cfg(not(feature = "console"))]
pub(crate) type ConsoleBound = ();

/// The table of flags the console shows (Settings), made from the command line before anything else is done with it.
#[cfg(feature = "console")]
pub(crate) type Flags = Vec<kafka_console::model::SettingsFlag>;
/// A build without the console has no table to show.
#[cfg(not(feature = "console"))]
pub(crate) type Flags = ();

/// Opens the broker, serves until told to stop, then stops cleanly: the server first, so nothing new arrives, then
/// the broker, which flushes every partition and records that it did.
pub(crate) async fn serve(
    settings: Settings,
    console_listener: ConsoleBound,
    flags: Flags,
) -> Result<()> {
    #[cfg(not(feature = "console"))]
    let () = flags;
    let Settings {
        broker,
        server,
        console,
        ..
    } = settings;
    // The request recorder is installed only when there is a console to read it: with `--console=false` the server carries no
    // observer and a request costs one `Option` test.
    #[cfg(feature = "console")]
    let recorder = console_listener
        .as_ref()
        .map(|_| kafka_console::Recorder::new());
    // The Lab: the fault table the server consults and the console makes rules in. Not installed with `--lab=false`: then a request
    // costs one `Option` test and no rule can exist.
    #[cfg(feature = "console")]
    let lab =
        (console.lab && console_listener.is_some()).then(kafka_server::fault::FaultTable::new);
    #[cfg(feature = "console")]
    let server = {
        let mut server = server;
        if let Some(r) = &recorder {
            server.observer = Some(r.clone());
        }
        if let Some(t) = &lab {
            server.fault = Some(t.clone());
            tracing::info!(
                "the Lab is on (--lab): rules that delay, fail or drop requests can be made from the console, are kept in memory only, and none exists now"
            );
        }
        server
    };
    #[cfg(not(feature = "console"))]
    let _ = (&console, console_listener);
    let acl_default_allow = broker.acl_default_allow;
    let broker = Arc::new(Broker::open(RealFs, broker).context("opening the broker's data")?);
    stored_acls(
        &console,
        &console_listener,
        acl_default_allow,
        broker.acl_count(),
    )?;
    users_ready(&server)?;
    // Asked to stop from now on means a clean stop: the handlers are registered before the broker says it is listening,
    // because a signal that arrives between that line and a late registration would end the process by the default action.
    let stop = stop_signal();
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
    #[cfg(feature = "console")]
    let web = console_listener
        .map(|bound| bound.serve(&console, &server, &running, &broker, (recorder, flags, lab)))
        .transpose()
        .context("starting the console")?;
    stop.await;
    tracing::info!("stopping");
    // The console first: nothing it shows or does outlives the broker it reads.
    #[cfg(feature = "console")]
    if let Some(web) = web {
        web.shutdown().await;
    }
    running.shutdown().await;
    broker.shutdown().await.context("stopping the broker")?;
    tracing::info!("stopped");
    Ok(())
}

/// Registers for SIGINT and SIGTERM now; the future resolves when the process is asked to stop.
fn stop_signal() -> impl std::future::Future<Output = ()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let signals = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        );
        async move {
            let (Ok(mut term), Ok(mut int)) = signals else {
                return std::future::pending().await;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
        }
    }
    #[cfg(not(unix))]
    {
        async {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

/// The users stored over the API are loaded once the broker's data is open, so only then is it known whether anybody can log in.
fn users_ready(server: &kafka_server::Config) -> Result<()> {
    if let Some(sasl) = &server.sasl {
        sasl.ensure_users()
            .map_err(|_| anyhow::anyhow!(crate::cli::NO_USERS))?;
        tracing::info!(
            configured = sasl.user_count(),
            stored = sasl.users().stored().user_count(),
            "SASL users"
        );
    }
    Ok(())
}

/// ACLs stored in the data directory are known only once it is open: a console served beyond loopback would get round them,
/// so that start is refused without `--console-unprotected`, and said with it (SEC-188).
#[cfg(feature = "console")]
fn stored_acls(
    console: &crate::cli::Console,
    bound: &ConsoleBound,
    acl_default_allow: bool,
    acls: usize,
) -> Result<()> {
    if bound.is_some()
        && let Some(note) = crate::cli::stored_acls(console, acl_default_allow, acls)?
    {
        tracing::warn!("{note}");
    }
    Ok(())
}

/// A build without the console has none to get round the ACLs.
#[cfg(not(feature = "console"))]
#[allow(clippy::unnecessary_wraps)]
fn stored_acls(_: &crate::cli::Console, (): &ConsoleBound, _: bool, _: usize) -> Result<()> {
    Ok(())
}
