//! doze-kafka: a Kafka broker for local development.

mod cli;
mod control;
mod descriptors;
mod duration;
mod lock;
mod run;
#[cfg(feature = "console")]
mod web;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let args = cli::Args::parse();
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
    let open = descriptors::open_partitions(
        settings.broker.max_open_partitions,
        descriptors,
        settings.server.max_connections,
    );
    if open != settings.broker.max_open_partitions {
        tracing::info!(
            "keeping at most {open} partitions open (the limit on open files is {descriptors})"
        );
        settings.broker.max_open_partitions = open;
    }
    settings.broker.max_open_descriptors =
        descriptors::open_files(descriptors, settings.server.max_connections);
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
    // The console's address is had before anything is created, the data directory and its lock file included (SEC-247):
    // one the operator chose that cannot be bound stops the start with exit 2, as every start that cannot be honoured does.
    #[cfg(feature = "console")]
    let console = match runtime.block_on(web::bind(&settings.console)) {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(2);
        }
    };
    #[cfg(not(feature = "console"))]
    let console = ();
    let _lock = lock::DataDirLock::take(&args.data_dir)?;
    runtime.block_on(run::serve(settings, console))
}
