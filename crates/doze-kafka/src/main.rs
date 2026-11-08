//! doze-kafka: a Kafka broker for local development.

mod cli;
mod duration;
mod lock;
mod run;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let args = cli::Args::parse();
    run::logging(&args);
    let settings = args.settings()?;
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
