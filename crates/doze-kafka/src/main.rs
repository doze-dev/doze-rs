//! doze-kafka: a Kafka broker for local development.

mod cli;
mod control;
mod descriptors;
mod duration;
mod lock;
mod run;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let args = cli::Args::parse();
    run::logging(&args);
    let mut settings = args.settings()?;
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
            users = sasl.user_count(),
            "SASL is on: clients must log in"
        );
    }
    #[cfg(feature = "tls")]
    if let Some(tls) = &settings.server.tls {
        // The certificate's file, never the key's content (nor the key file's name: it says nothing a log needs).
        tracing::info!(cert = %tls.cert_file().display(), "TLS is on: clients must connect with TLS (1.2 or newer)");
    }
    for flag in args.ignored() {
        tracing::warn!(
            "{flag} is accepted and ignored: that part of doze-kafka is not in this edition yet"
        );
    }
    let _lock = lock::DataDirLock::take(&args.data_dir)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.worker_threads)
        .thread_name("doze-kafka")
        .enable_all()
        .build()?;
    runtime.block_on(run::serve(settings))
}
