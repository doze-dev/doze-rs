//! doze-aws: local AWS for development.
//!
//! `doze-aws [flags]` serves; `doze-aws <command>` runs one of the commands (`env`, `apply`, `export`, `doctor`, `config print`,
//! `version`, `help`). A word that is not a command is an error with usage, never a server started by accident.

mod apply;
mod cfn;
mod cli;
mod client;
mod config;
mod doctor;
mod env;
mod events;
mod export;
mod instance;
mod keys;
mod locate;
mod probe;
mod ready;
mod run;
mod services;
mod show;
mod startfail;
mod style;
mod toml;
mod wire;

use std::process::ExitCode;
use std::time::Instant;

use clap::FromArgMatches;

use crate::cli::{Cli, Command, ConfigCommand};

/// This build's version (`DOZE_AWS_VERSION` at build time, else the crate's).
pub(crate) const VERSION: &str = match option_env!("DOZE_AWS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

fn main() -> ExitCode {
    let started = Instant::now();
    let matches = cli::command().get_matches();
    let parsed = match Cli::from_arg_matches(&matches) {
        Ok(p) => p,
        Err(e) => e.exit(),
    };
    if parsed.version || matches!(parsed.command, Some(Command::Version)) {
        print!("{}", show::version());
        return ExitCode::SUCCESS;
    }
    let sub = match matches.subcommand() {
        Some(("config", m)) => m.subcommand().map(|(_, p)| p),
        Some((_, m)) => Some(m),
        None => None,
    };
    let (given, file) = cli::given(&matches, sub);
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = match config::Config::load(&given, file.as_deref(), &cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("doze-aws: {e}");
            return ExitCode::from(2);
        }
    };
    let code = match parsed.command {
        None => run::main(&cfg, started),
        Some(Command::Env(args)) => env::run(&args, &cfg),
        Some(Command::Config(ConfigCommand::Print)) => {
            print!("{}", show::config(&cfg));
            0
        }
        Some(Command::Apply(args)) => apply::run(args, &cfg),
        Some(Command::Export(args)) => export::run(&args, &cfg),
        Some(Command::Doctor) => doctor::run(&cfg),
        Some(Command::Version) => 0,
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
