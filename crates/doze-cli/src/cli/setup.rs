//! The arguments of the commands that set a project up, open the app, look after modules, and the rest.

use super::args::DryRun;
use super::values::Shell;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct Init {
    /// Ask instead of reading the project
    #[arg(long)]
    pub wizard: bool,
    /// Overwrite an existing doze.hcl
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Add {
    /// A module (valkey, postgres, …) or `service`
    #[arg(value_name = "module|service")]
    pub what: String,
    /// The block's name (default: the module's usual one)
    #[arg(value_name = "name")]
    pub name: Option<String>,
    /// Engine version (default: the newest the module offers)
    #[arg(long, value_name = "v")]
    pub version: Option<String>,
    /// The port to declare (default: the module's usual one)
    #[arg(long, value_name = "n")]
    pub port: Option<u16>,
    /// For a service: how it is run
    #[arg(long = "as", value_name = "how", value_parser = ["installed", "container", "download"])]
    pub run_as: Option<String>,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Lint {
    /// Check only what needs no module (no cache or network)
    #[arg(long)]
    pub offline: bool,
}

#[derive(Debug, Args)]
pub struct Fmt {
    /// Write nothing; list the files that would change and exit 1 if any would
    #[arg(long)]
    pub check: bool,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Doctor {
    /// Run the one-time machine setup for .doze names (one sudo)
    #[arg(long)]
    pub fix: bool,
    /// Show the setup script instead of running it (with --fix)
    #[arg(long)]
    pub dry_run: bool,
    /// The published spelling of --fix --dry-run
    #[arg(long, hide = true)]
    pub print_fix: bool,
}

#[derive(Debug, Args)]
pub struct DnsSetup {
    /// Report what is set up (no sudo)
    #[arg(long)]
    pub check: bool,
    /// Remove the setup (needs sudo)
    #[arg(long)]
    pub uninstall: bool,
    /// Show the privileged script instead of running it
    #[arg(long, alias = "print")]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct Trust {
    /// Report what is set up (no sudo)
    #[arg(long)]
    pub check: bool,
    /// Remove this stack's routes, and the authority's trust with the last stack (needs sudo)
    #[arg(long)]
    pub remove: bool,
    /// Make a new certificate authority and trust it
    #[arg(long)]
    pub rotate: bool,
    /// Show the privileged script instead of running it
    #[arg(long, alias = "print")]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct Open {
    /// A project directory, or a doze:// link to a stack or service
    #[arg(value_name = "dir|link")]
    pub target: Option<String>,
    /// Open the window at this service
    #[arg(long, value_name = "service")]
    pub show: Option<String>,
    /// Open this service's web console (temporal's Web UI, the aws console) in the browser
    #[arg(long, value_name = "name", conflicts_with = "show")]
    pub console: Option<String>,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Modules {
    #[command(subcommand)]
    pub command: Option<ModulesCmd>,
}

#[derive(Debug, Subcommand)]
pub enum ModulesCmd {
    /// Move module pins in doze.lock to the newest releases that fit
    Upgrade(Upgrade),
    /// A module's documentation: what it runs, an example, every argument
    Docs(Docs),
    /// Re-check every pin in doze.lock against the signed registry index
    Verify,
}

#[derive(Debug, Args)]
pub struct Upgrade {
    /// Modules to move (default: all)
    #[arg(value_name = "module")]
    pub modules: Vec<String>,
    /// Report what would move without changing doze.lock (exit 1 if any would)
    #[arg(long)]
    pub check: bool,
    #[command(flatten)]
    pub dry: DryRun,
}

#[derive(Debug, Args)]
pub struct Docs {
    /// valkey, postgres, … (or namespace/module)
    #[arg(value_name = "module")]
    pub module: String,
    /// Open its page on the registry site instead
    #[arg(long)]
    pub web: bool,
}

#[derive(Debug, Args)]
pub struct Completion {
    #[arg(value_name = "shell")]
    pub shell: Shell,
}

#[derive(Debug, Args)]
pub struct Refresh {
    /// Fetch the release file
    #[arg(long)]
    pub latest: bool,
    /// Fetch this module's index and documentation
    #[arg(long = "module", value_name = "source")]
    pub modules: Vec<String>,
}
