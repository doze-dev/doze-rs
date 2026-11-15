//! `doze-kafka data`: what is using the disk, and giving it back, from the command line (docs/port/data-control.md).
//!
//! - `doze-kafka data` says what the data directory holds. With the broker stopped it opens the directory as the broker does
//!   (taking its lock) and says each topic's size and why; while the broker runs it reads the files alone, opening nothing
//!   for writing and never the metadata store the broker holds, and points to the console for the rest.
//! - `doze-kafka data empty --topic NAME` and `--all-topics`, and `doze-kafka data erase --everything`, need the broker
//!   stopped (they take its lock, so they cannot run beside it) and do what the console's Data page does. The two that reach
//!   every topic say what they would do and stop unless given `--yes`.

mod report;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Config, DataRefusal, bytes_words};
use kafka_storage::fs::RealFs;

/// What is using the disk, and giving it back.
#[derive(Debug, Parser)]
#[command(
    name = "doze-kafka data",
    about = "What is using the disk, and giving it back: a topic emptied, every topic emptied, or everything erased."
)]
pub(crate) struct DataArgs {
    /// The broker's data directory.
    #[arg(long, default_value = "./data", global = true)]
    data_dir: PathBuf,
    #[command(subcommand)]
    action: Option<Action>,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Remove every record of a topic, or of every topic, and free the disk now. Topics, partitions, configs and committed
    /// offsets stay; each partition carries on from its end. The broker must be stopped.
    Empty {
        /// The topic to empty.
        #[arg(
            long,
            conflicts_with = "all_topics",
            required_unless_present = "all_topics"
        )]
        topic: Option<String>,
        /// Every topic.
        #[arg(long)]
        all_topics: bool,
        /// Go ahead with --all-topics (without it, say what would be emptied and stop).
        #[arg(long)]
        yes: bool,
    },
    /// Delete every topic, group, transactional id and schema: the broker starts empty. ACLs, users and the broker's settings
    /// stay. The broker must be stopped.
    Erase {
        /// Everything (the only thing erase does: the flag says it out loud).
        #[arg(long, required = true)]
        everything: bool,
        /// Go ahead (without it, say what would be erased and stop).
        #[arg(long)]
        yes: bool,
    },
}

/// Whether this process was started as `doze-kafka data`.
pub(crate) fn invoked() -> bool {
    std::env::args_os().nth(1).is_some_and(|a| a == "data")
}

/// Runs `doze-kafka data`: exit 0 when done, 1 when refused or failed, 2 for a command line clap refuses.
pub(crate) fn run() -> std::process::ExitCode {
    let args = DataArgs::parse_from(std::env::args_os().skip(1));
    match execute(&args, &mut std::io::stdout()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Does what `args` asks, writing to `out`.
///
/// # Errors
/// A refusal, a broker that is running for an action that needs it stopped, or a failure to open the data directory.
pub(crate) fn execute(args: &DataArgs, out: &mut dyn std::io::Write) -> Result<()> {
    let dir = &args.data_dir;
    if !dir.is_dir() {
        bail!("there is no data directory at {}", dir.display());
    }
    let Some(lock) = lock(dir)? else {
        if args.action.is_some() {
            bail!(
                "doze-kafka is running on {}: empty or erase from the console's Data page (/data), or stop the broker and run this again",
                dir.display()
            );
        }
        return report::running(dir, out);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let done = runtime.block_on(on_stopped(dir, args.action.as_ref(), out));
    drop(lock);
    done
}

/// The data directory's lock, or `None` if a broker holds it.
fn lock(dir: &Path) -> Result<Option<doze_runtime::DataDirLock>> {
    match doze_runtime::DataDirLock::take(dir, "doze-kafka") {
        Ok(lock) => Ok(Some(lock)),
        Err(doze_runtime::LockError::Held { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The operator, as the console acts: a super user of every broker.
fn operator() -> Caller {
    Caller::new(Principal::console(), None)
}

/// With the broker stopped: open the directory as the broker would, do it, and stop cleanly.
async fn on_stopped(
    dir: &Path,
    action: Option<&Action>,
    out: &mut dyn std::io::Write,
) -> Result<()> {
    let broker = Arc::new(
        Broker::open(RealFs, Config::new(dir.to_path_buf()))
            .with_context(|| format!("opening {}", dir.display()))?,
    );
    let done = act(&broker, action, out).await;
    broker.shutdown().await?;
    done
}

async fn act(broker: &Broker, action: Option<&Action>, out: &mut dyn std::io::Write) -> Result<()> {
    let caller = operator();
    let Some(usage) = broker.data_usage(&caller) else {
        bail!("the data directory could not be read");
    };
    let subjects = registry::subjects(broker);
    match action {
        None => report::stopped(&usage, subjects, out),
        Some(Action::Empty {
            topic: Some(topic), ..
        }) => {
            let e = broker.empty_topic(&caller, topic).await.map_err(refused)?;
            writeln!(
                out,
                "Emptied {}: {} in {}, {} freed. It carries on from where it ended.",
                e.topic,
                count(e.offsets, "record"),
                count(e.partitions, "partition"),
                bytes_words(e.freed_bytes)
            )?;
            Ok(())
        }
        Some(Action::Empty { yes: false, .. }) => {
            writeln!(
                out,
                "This would empty {} and free about {}. Topics, partitions, configs and committed offsets stay.\nRun it again with --yes to go ahead.",
                count(usage.topics.len(), "topic"),
                bytes_words(usage.topics_bytes())
            )?;
            bail!("nothing was emptied: --yes was not given")
        }
        Some(Action::Empty { .. }) => {
            let e = broker.empty_topics(&caller).await.map_err(refused)?;
            writeln!(
                out,
                "Emptied {}: {}, {} freed.",
                count(e.topics.len(), "topic"),
                count(e.offsets(), "record"),
                bytes_words(e.freed_bytes())
            )?;
            Ok(())
        }
        Some(Action::Erase { yes: false, .. }) => {
            writeln!(
                out,
                "This would delete {} ({}), the committed offsets of {}, {}{}.\nACLs, users and the broker's settings stay. Run it again with --yes to go ahead.",
                count(usage.topics.len(), "topic"),
                bytes_words(usage.topics_bytes() + usage.stray_bytes()),
                count(usage.groups, "group"),
                count(usage.transactional_ids, "transactional id"),
                subjects.map_or_else(String::new, |n| format!(" and {n} schema subjects")),
            )?;
            bail!("nothing was erased: --yes was not given")
        }
        Some(Action::Erase { .. }) => {
            let e = broker.erase_everything(&caller).await.map_err(refused)?;
            let gone = registry::erase(broker)?;
            writeln!(
                out,
                "Erased {}, {}, {}{}; {} freed. ACLs, users and the broker's settings are as they were.",
                count(e.topics, "topic"),
                count(e.groups, "group"),
                count(e.transactional_ids, "transactional id"),
                gone.map_or_else(String::new, |n| format!(", {n} schema subjects")),
                bytes_words(e.freed_bytes)
            )?;
            Ok(())
        }
    }
}

/// `1 record`, `12 records`.
fn count<N: std::fmt::Display + PartialEq + From<u8>>(n: N, one: &str) -> String {
    if n == N::from(1) {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

fn refused(r: DataRefusal) -> anyhow::Error {
    anyhow::anyhow!(r.sentence())
}

/// The schema registry's subjects, kept in the metadata store beside the broker's records.
#[cfg(feature = "registry")]
mod registry {
    use kafka_broker::Broker;
    use kafka_registry::Registry;
    use kafka_registry::store::MetaStorage;
    use kafka_storage::fs::RealFs;

    fn open(broker: &Broker<RealFs>) -> Option<Registry> {
        Registry::open(Box::new(MetaStorage(std::sync::Arc::clone(broker.meta())))).ok()
    }

    pub(super) fn subjects(broker: &Broker<RealFs>) -> Option<usize> {
        open(broker).map(|r| r.subjects("", true, false).len())
    }

    pub(super) fn erase(broker: &Broker<RealFs>) -> anyhow::Result<Option<usize>> {
        let Some(r) = open(broker) else {
            return Ok(None);
        };
        Ok(Some(
            r.erase()
                .map_err(|e| anyhow::anyhow!("the schema registry: {e}"))?
                .subjects,
        ))
    }
}

/// A build without the registry has no subjects to say or erase.
#[cfg(not(feature = "registry"))]
mod registry {
    use kafka_broker::Broker;
    use kafka_storage::fs::RealFs;

    pub(super) fn subjects(_: &Broker<RealFs>) -> Option<usize> {
        None
    }

    pub(super) fn erase(_: &Broker<RealFs>) -> anyhow::Result<Option<usize>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
