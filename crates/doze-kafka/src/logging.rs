//! Logging to stderr, filtered by `RUST_LOG` or `--log-level`.
//!
//! The filter is tracing-subscriber's `Targets`: a default level and a level per target (a module path prefix), and no
//! regex engine. `EnvFilter`'s span and field directives (`target[span{field=value}]=level`) need one (`regex`,
//! `regex-automata`, `regex-syntax`, `matchers`: about 230 KB of the binary, docs/port/weight.md) and are refused by name at
//! start rather than read as something else.

use std::str::FromStr;

use tracing_subscriber::Layer;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::cli::{Args, LogFormat};

/// What a filter may say, for the message that refuses one.
const SUPPORTED: &str = concat!(
    "give a level (error, warn, info, debug, trace or off) or a comma-separated list of levels ",
    "and target=level pairs (warn,kafka_broker=debug)"
);

/// Sets up logging to stderr: `RUST_LOG`, if it is set and not empty, else `--log-level`.
///
/// # Errors
/// A filter this build cannot apply, as one line naming where it came from and what is supported.
pub(crate) fn init(args: &Args) -> Result<(), String> {
    let env = std::env::var("RUST_LOG");
    let (source, spec) = match &env {
        Ok(spec) if !spec.trim().is_empty() => ("RUST_LOG", spec),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!("RUST_LOG is not valid UTF-8; {SUPPORTED}"));
        }
        _ => ("--log-level", &args.log_level),
    };
    let filter = parse(source, spec)?;
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr);
    let registry = tracing_subscriber::registry();
    match args.log_format {
        LogFormat::Text => registry.with(layer.with_filter(filter)).init(),
        LogFormat::Json => registry.with(layer.json().with_filter(filter)).init(),
    }
    Ok(())
}

/// [`filter`], refused in one line that names where `spec` came from and what is supported.
fn parse(source: &str, spec: &str) -> Result<Targets, String> {
    filter(spec).map_err(|why| format!("{source} {spec:?}: {why}; {SUPPORTED}"))
}

/// The filter `spec` says: comma-separated directives, each a level (the default for every target), `target=level`, or a
/// bare target (every level of it), as `RUST_LOG` has always read them. Empty directives are skipped.
fn filter(spec: &str) -> Result<Targets, String> {
    let mut targets = Targets::new();
    for directive in spec.split(',').map(str::trim).filter(|d| !d.is_empty()) {
        if directive.contains(['[', ']', '{', '}']) {
            return Err(format!(
                "span and field filters ({directive}) are not supported"
            ));
        }
        targets = match directive.split_once('=') {
            Some((target, level)) => {
                let (target, level) = (target.trim(), level.trim());
                if target.is_empty() {
                    return Err(format!("{directive} names no target"));
                }
                targets.with_target(target, self::level(level)?)
            }
            None => match LevelFilter::from_str(directive) {
                Ok(level) => targets.with_default(level),
                Err(_) => targets.with_target(directive, LevelFilter::TRACE),
            },
        };
    }
    Ok(targets)
}

fn level(level: &str) -> Result<LevelFilter, String> {
    LevelFilter::from_str(level).map_err(|_| format!("{level} is not a level"))
}

#[cfg(test)]
mod tests;
