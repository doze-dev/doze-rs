//! Starting the broker, and stopping it when asked.

use std::sync::Arc;

use anyhow::{Context, Result};
use kafka_broker::Broker;
use kafka_server::Server;
use kafka_storage::fs::RealFs;

use crate::cli::Settings;

/// The console's listener as `main` had it bound before anything was created.
#[cfg(feature = "console")]
pub(crate) type ConsoleBound = Option<crate::web::Bound>;
/// A build without the console has none to bind.
#[cfg(not(feature = "console"))]
pub(crate) type ConsoleBound = ();

/// The schema registry's listener as `main` had it bound before anything was created.
#[cfg(feature = "registry")]
pub(crate) type RegistryBound = Result<crate::registry::Bound, crate::registry::Off>;
/// A build without the registry has none to bind.
#[cfg(not(feature = "registry"))]
pub(crate) type RegistryBound = ();

#[cfg(feature = "registry")]
use crate::registry::{start as start_registry, stop as stop_registry};

/// A build without the registry starts none.
#[cfg(not(feature = "registry"))]
#[allow(clippy::unnecessary_wraps)]
fn start_registry(
    (): RegistryBound,
    _: &kafka_server::Config,
    _: &Arc<Broker<RealFs>>,
) -> Result<(Option<()>, ())> {
    Ok((None, ()))
}

/// What the console is told of the registry: in a build without one, that it is not in this build.
#[cfg(all(feature = "console", feature = "registry"))]
fn console_schemas(s: &crate::registry::Status) -> kafka_console::schemas::Schemas {
    s.console()
}
#[cfg(all(feature = "console", not(feature = "registry")))]
fn console_schemas((): &()) -> kafka_console::schemas::Schemas {
    kafka_console::schemas::Schemas::default()
}

/// A build without the registry has none to stop.
#[cfg(not(feature = "registry"))]
async fn stop_registry(_: Option<()>) {}

/// The table of flags the console shows (Settings), made from the command line before anything else is done with it.
#[cfg(feature = "console")]
pub(crate) type Flags = Vec<kafka_console::model::SettingsFlag>;
/// A build without the console has no table to show.
#[cfg(not(feature = "console"))]
pub(crate) type Flags = ();

/// The server with the console's recorder and the Lab's fault table on it, where there are.
#[cfg(feature = "console")]
fn observed(
    mut server: kafka_server::Config,
    recorder: Option<&Arc<kafka_console::Recorder>>,
    lab: Option<&Arc<kafka_server::fault::FaultTable>>,
) -> kafka_server::Config {
    if let Some(r) = recorder {
        server.observer = Some(r.clone());
    }
    if let Some(t) = lab {
        server.fault = Some(t.clone());
        tracing::info!(
            "the Lab is on (--lab): rules that delay, fail or drop requests can be made from the console, are kept in memory only, and none exists now"
        );
    }
    server
}

/// Opens the broker, serves until told to stop, then stops cleanly: the server first, so nothing new arrives, then
/// the broker, which flushes every partition and records that it did.
pub(crate) async fn serve(
    settings: Settings,
    (console_listener, registry_listener): (ConsoleBound, RegistryBound),
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
    let server = observed(server, recorder.as_ref(), lab.as_ref());
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
    let _sweepers = (broker.start_offsets_sweeper(), broker.start_token_sweeper());
    let _transactions = broker.start_transactions();
    let _share_locks = broker.start_share_locks();
    let _logs = broker.start_log_sweeper();
    let _cleaner = broker.start_log_cleaner();
    let _closer = broker.start_partition_closer();
    let _control = crate::control::serve(&broker);
    // The registry before the console: the console reads it in this process.
    let (schemas, registry_status) = start_registry(registry_listener, &server, &broker)?;
    #[cfg(not(feature = "console"))]
    let _ = &registry_status;
    #[cfg(feature = "console")]
    let web = console_listener
        .map(|bound| {
            bound.serve(
                &console,
                &server,
                &running,
                &broker,
                (recorder, flags, lab, console_schemas(&registry_status)),
            )
        })
        .transpose()
        .context("starting the console")?;
    stop.await;
    tracing::info!("stopping");
    stop_registry(schemas).await;
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
