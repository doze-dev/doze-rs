//! Each command's own arguments and flags. `--dry-run` is on every command that changes something (the owner's
//! decision); `--print` and `--print-fix` stay as hidden spellings of it so the published docs keep working.

use clap::Args;
use std::ffi::OsString;
use std::time::Duration;

/// Only `--dry-run`.
#[derive(Debug, Args)]
pub struct DryRun {
    /// Show what it would do; change nothing
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct Up {
    /// Services to bring up (default: what you pick, or everything)
    #[arg(value_name = "service", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub services: Vec<String>,
    /// Stream logs after waking (Ctrl-C detaches; the stack keeps running)
    #[arg(short, long)]
    pub follow: bool,
    /// Start what was started last time, without asking
    #[arg(long, conflicts_with = "all")]
    pub last: bool,
    /// Start everything, without asking
    #[arg(long)]
    pub all: bool,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Sync {
    /// Only this service
    #[arg(value_name = "service", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub service: Option<String>,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Status {
    /// Keep the table on screen and redraw it as things change (Ctrl-C to stop)
    #[arg(short, long)]
    pub watch: bool,
    /// Draw the dependency tree instead of the table
    #[arg(long)]
    pub graph: bool,
}

#[derive(Debug, Args)]
pub struct Env {
    #[arg(value_name = "service", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub services: Vec<String>,
    /// KEY=value lines for a .env file
    #[arg(long)]
    pub dotenv: bool,
}

#[derive(Debug, Args)]
pub struct Run {
    /// Set the `doze env` variables in the command's environment
    #[arg(long)]
    pub env: bool,
    /// The command and its arguments, after --
    #[arg(last = true, required = true, value_name = "command")]
    pub command: Vec<OsString>,
}

/// Service names; none means all.
#[derive(Debug, Args)]
pub struct Names {
    /// Services to act on (none: every one)
    #[arg(value_name = "name", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub names: Vec<String>,
    #[command(flatten)]
    pub dry: DryRun,
}

/// At least one service name.
#[derive(Debug, Args)]
pub struct SomeNames {
    /// Services to act on
    #[arg(value_name = "name", required = true, add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub names: Vec<String>,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct OneName {
    /// The service
    #[arg(value_name = "name", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub name: String,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Task {
    /// The task to run (none: list them)
    #[arg(value_name = "name", add = clap_complete::ArgValueCandidates::new(crate::complete::tasks))]
    pub name: Option<String>,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Pin {
    /// Services to keep awake
    #[arg(value_name = "name", required = true, add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub names: Vec<String>,
    /// Unpin: let them sleep again when idle
    #[arg(long, conflicts_with = "toggle")]
    pub off: bool,
    /// Flip each one, as the app's pin switch does
    #[arg(long)]
    pub toggle: bool,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Logs {
    /// Services whose output to show (none: every one, merged by time)
    #[arg(value_name = "name", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub names: Vec<String>,
    /// Keep printing as lines are written (Ctrl-C to stop)
    #[arg(short, long)]
    pub follow: bool,
    /// Which run: 0 the newest, -1 the one before, N a run by number
    #[arg(
        long,
        value_name = "n",
        allow_negative_numbers = true,
        default_value_t = 0
    )]
    pub run: i64,
    /// How many lines from the end of the run (default 200)
    #[arg(short = 'n', long, value_name = "n")]
    pub tail: Option<u32>,
    /// Only lines newer than this long ago, e.g. 10m
    #[arg(long, value_name = "duration", value_parser = super::values::duration)]
    pub since: Option<Duration>,
}

/// One service, by name.
#[derive(Debug, Args)]
pub struct Service {
    /// The service
    #[arg(value_name = "name", add = clap_complete::ArgValueCandidates::new(crate::complete::services))]
    pub name: String,
}
