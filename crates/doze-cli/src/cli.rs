//! The command surface (proposal, "The command surface" and "Command coverage"): every command, alias and flag, with
//! help that says what each does in plain words. Groups for `--help` are in [`crate::help`].

pub mod args;
mod examples;
pub mod setup;
pub mod values;

use args::*;
use clap::{Args, Parser, Subcommand};
use setup::*;
use std::path::PathBuf;
use std::time::Duration;

/// `doze`.
#[derive(Debug, Parser)]
#[command(
    name = "doze",
    disable_version_flag = true,
    about = "Real local services without Docker: each wakes on its first connection and sleeps when idle",
    args_conflicts_with_subcommands = false
)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

/// The flags every command takes.
#[derive(Debug, Args)]
#[command(next_help_heading = "Flags for every command")]
pub struct Global {
    /// Path to doze.hcl (a directory: its doze.hcl); found here or in a parent when not given
    #[arg(short = 'c', long, global = true, value_name = "path")]
    pub config: Option<PathBuf>,
    /// Set a config variable (repeatable)
    #[arg(long = "var", global = true, value_name = "name=value", value_parser = values::var)]
    pub vars: Vec<(String, String)>,
    /// Print the result as one JSON envelope, doze/v1 (DOZE_JSON=1)
    #[arg(long, global = true)]
    pub json: bool,
    /// Print nothing on stdout: the exit code is the answer
    #[arg(short, long, global = true)]
    pub quiet: bool,
    /// Answer yes to every question (DOZE_YES=1)
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
    /// How long to wait for one service to become ready
    #[arg(long, global = true, value_name = "duration", value_parser = values::duration, default_value = "5m")]
    pub timeout: Duration,
    /// No colour (NO_COLOR=1)
    #[arg(long, global = true)]
    pub no_color: bool,
}

/// Every command. The order here is the order within each help group.
#[derive(Debug, Subcommand)]
pub enum Cmd {
    // The stack
    /// Bring the stack up in the background: converge it with doze.hcl, then wake every service
    Up(Up),
    /// Put every service to sleep and stop doze, then check nothing is left running
    Down(DryRun),
    /// Converge the stack with doze.hcl: create what is new, change what changed, remove what went
    Sync(Sync),
    /// Every service: its state, its address and what it uses
    #[command(after_help = examples::STATUS)]
    #[command(visible_aliases = ["tree", "ls", "ps"])]
    Status(Status),
    /// The services' connection variables, as exports to eval (or lines for a .env)
    Env(Env),
    /// Run a command with the stack up (with --env, its variables set)
    Run(Run),

    // One service
    /// Wake services now, with what they depend on (no name: everything)
    #[command(after_help = examples::WAKE)]
    #[command(visible_alias = "boot")]
    Wake(Names),
    /// Put services to sleep, keeping their data (no name: everything awake)
    #[command(after_help = examples::SLEEP)]
    #[command(visible_alias = "reap")]
    Sleep(Names),
    /// Stop services and start them again
    #[command(after_help = examples::RESTART)]
    Restart(SomeNames),
    /// Delete services' data; each starts fresh on its next wake
    Reset(SomeNames),
    /// Move a postgres service's data to the major version doze.hcl now declares
    Migrate(OneName),
    /// Run a task now, or list the tasks and how they last ran
    #[command(after_help = examples::TASK)]
    Task(Task),
    /// Keep services awake past their idle time (--off lets them sleep again)
    #[command(after_help = examples::PIN)]
    #[command(visible_alias = "keepawake")]
    Pin(Pin),
    /// Take services out of this run: stop them, and nothing wakes them until doze on
    Off(SomeNames),
    /// Put services back into this run; each wakes on its next connection
    On(SomeNames),
    /// What services printed, also while asleep, after a crash, or with doze down
    #[command(after_help = examples::LOGS)]
    Logs(Logs),
    /// A service's connect line (pipe it to your clipboard)
    #[command(visible_alias = "copy")]
    Url(Service),
    /// Open the engine's own client on a service (valkey-cli, psql), signed in
    Connect(Service),
    /// Why a service is awake or asleep: what woke it, what keeps it, when it sleeps
    #[command(after_help = examples::WHY)]
    Why(Service),

    // Set up and edit
    /// Write a doze.hcl for this project from what it finds, saying why (or a wizard)
    Init(Init),
    /// Add a module or a service to doze.hcl
    Add(Add),
    /// Check doze.hcl without touching the daemon or any data
    Lint(Lint),
    /// Format doze.hcl and its sibling *.doze.hcl files
    Fmt(Fmt),
    /// Check this machine and project for what would stop doze, and say how to fix it
    #[command(after_help = examples::DOCTOR)]
    Doctor(Doctor),
    /// One-time machine setup for .doze names (the same as doze doctor --fix)
    #[command(after_help = examples::DNS_SETUP)]
    DnsSetup(DnsSetup),
    /// Set this machine up for the stack's own domains: routes and https (one sudo)
    #[command(after_help = examples::TRUST)]
    Trust(Trust),

    // The app
    /// Open the doze app at this project, or a module's web console (--console)
    Open(Open),

    // Modules and the lockfile
    /// The modules the stack uses: each pin in doze.lock and the newest release that fits
    #[command(visible_alias = "mod")]
    Modules(Modules),

    // Other
    /// Print the doze version
    Version,
    /// Update doze the way it was installed, without stopping services
    Update(DryRun),
    /// Write a bug report (versions, the stack's shape, recent logs) to a file
    Bug,
    /// Print the completion script for a shell
    Completion(Completion),

    /// The background refresh: the release file and module documentation, within the network budget.
    #[command(name = "__refresh", hide = true)]
    Refresh(Refresh),
}
