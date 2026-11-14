//! doze-kafka: a Kafka broker for local development.

mod cli;
mod control;
mod descriptors;
mod duration;
#[cfg(feature = "console")]
mod facts;
#[cfg(feature = "module")]
mod module;
mod run;
#[cfg(feature = "console")]
mod web;

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches};

/// `Parser::parse`, taken apart: the matches say which flags were given, which Settings shows.
fn parse() -> (cli::Args, run::Flags) {
    let mut cmd = cli::Args::command();
    let matches = cmd.get_matches_mut();
    let args = cli::Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    #[cfg(feature = "console")]
    let flags = facts::flags(&cmd, &matches);
    #[cfg(not(feature = "console"))]
    #[allow(clippy::let_unit_value)]
    let flags = ();
    (args, flags)
}

fn main() -> std::process::ExitCode {
    // Started by doze as its kafka module (`kafka-plugin`), or as `doze-kafka module`: docs/port/phase11.md.
    #[cfg(feature = "module")]
    if module::invoked() {
        return module::run();
    }
    match broker() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // What returning the error from `main` prints, and its exit code.
        Err(e) => {
            eprintln!("Error: {e:?}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn broker() -> Result<()> {
    let (args, flags) = parse();
    run::logging(&args);
    // A configuration that cannot start exits 2, as a bad flag does (clap's own code), before anything is opened.
    let mut settings = match args.settings() {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(2);
        }
    };
    let descriptors = descriptors::raise();
    // Every connection that may be open holds a descriptor, the console's included (a stream is one).
    let connections = descriptors::connections(&settings);
    let open = descriptors::open_partitions(
        settings.broker.max_open_partitions,
        descriptors,
        connections,
    );
    if open != settings.broker.max_open_partitions {
        tracing::info!(
            "keeping at most {open} partitions open (the limit on open files is {descriptors})"
        );
        settings.broker.max_open_partitions = open;
    }
    settings.broker.max_open_descriptors = descriptors::open_files(descriptors, connections);
    if let Some(sasl) = &settings.server.sasl {
        let offered: Vec<_> = sasl.offered().iter().map(|m| m.name()).collect();
        tracing::info!(
            mechanisms = %offered.join(","),
            configured_users = sasl.user_count(),
            "SASL is on: clients must log in"
        );
    }
    #[cfg(feature = "tls")]
    if let Some(tls) = &settings.server.tls {
        // The certificate's file, never the key's content (nor the key file's name: it says nothing a log needs).
        tracing::info!(cert = %tls.cert_file().display(), "TLS is on: clients must connect with TLS (1.2 or newer)");
    }
    for flag in args.ignored() {
        if flag.starts_with("--console") {
            tracing::warn!(
                "{flag} is accepted and ignored: this build has no console (it is built without the `console` feature)"
            );
        } else {
            tracing::warn!(
                "{flag} is accepted and ignored: that part of doze-kafka is not in this edition yet"
            );
        }
    }
    for warning in &settings.warnings {
        tracing::warn!("{warning}");
    }
    tracing::debug!(
        console = settings.console.enabled,
        wanted = ?settings.console.addr,
        prefix = %settings.console.prefix,
        allow_hosts = ?settings.console.allow_hosts,
        unprotected = settings.console.unprotected,
        "console flags"
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.worker_threads)
        .thread_name("doze-kafka")
        .enable_all()
        .build()?;
    // (In a build without the console this is the unit value: there is nothing to bind.)
    #[cfg_attr(not(feature = "console"), allow(clippy::let_unit_value))]
    let console = bind_console(&runtime, &settings);
    let _lock = doze_runtime::DataDirLock::take(&args.data_dir, "doze-kafka")?;
    runtime.block_on(run::serve(settings, console, flags))
}

/// The console's address is had before anything is created, the data directory and its lock file included (SEC-247): one the
/// operator chose that cannot be bound stops the start with exit 2, as every start that cannot be honoured does.
#[cfg(feature = "console")]
fn bind_console(runtime: &tokio::runtime::Runtime, settings: &cli::Settings) -> run::ConsoleBound {
    match runtime.block_on(web::bind(&settings.console)) {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(2);
        }
    }
}

/// A build without the console binds none.
#[cfg(not(feature = "console"))]
fn bind_console(_: &tokio::runtime::Runtime, _: &cli::Settings) -> run::ConsoleBound {}
