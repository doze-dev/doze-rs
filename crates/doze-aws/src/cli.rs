//! The command line: `doze-aws [flags]` serves; `doze-aws <command>` runs a command. An unknown word is an error with usage (clap
//! suggests the nearest command), never a server started by accident.

use std::path::PathBuf;

use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Args, CommandFactory, Parser, Subcommand};

use crate::config::{Given, Source};
use crate::keys::{KEYS, Kind, env_var};

const EXAMPLES: &str = "\
Examples:
  doze-aws                                 serve every service on 127.0.0.1:4566
  doze-aws --services sqs,s3,lambda        serve just those three
  doze-aws --listen 127.0.0.1:0            serve on any free port
  eval \"$(doze-aws env)\"                   point this shell's AWS SDKs at it
  doze-aws apply                           create or update a stack from ./template.yaml
  doze-aws doctor                          check what could be in the way

Settings: flag > DOZE_AWS_<NAME> > doze-aws.toml > default. `doze-aws config print` shows them all.";

/// Local AWS for development: one binary, every service, no Docker.
#[derive(Debug, Parser)]
#[command(name = "doze-aws", disable_version_flag = true, after_help = EXAMPLES)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    /// Print the version and the services this build serves.
    #[arg(short = 'V', long)]
    pub(crate) version: bool,
}

/// The commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Print the version and the services this build serves.
    Version,
    /// Print the shell lines that point AWS SDKs and the AWS CLI at the running doze-aws.
    Env(EnvArgs),
    /// The configuration.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Create or update a CloudFormation stack on the running doze-aws, following its events as they happen.
    Apply(ApplyArgs),
    /// Print the template a stack on the running doze-aws was made from.
    Export(ExportArgs),
    /// Check what could get in the way, and say what fixes it.
    Doctor,
    /// What doze-aws keeps on disk, and removing it: one resource, one service, or everything.
    #[command(args_conflicts_with_subcommands = true, after_help = DATA_EXAMPLES)]
    Data(DataArgs),
    /// Every AWS operation this build answers, as JSON (what the compatibility page is made from).
    #[command(hide = true)]
    Compat,
}

const DATA_EXAMPLES: &str = "\
Examples:
  doze-aws data                              where the space goes, service by service
  doze-aws data s3                           every bucket, largest first
  doze-aws data s3 uploads                   one bucket, and what is in it that you may not know about
  doze-aws data empty sqs orders-dlq         remove a queue's messages, keep the queue
  doze-aws data empty s3 uploads --prefix tmp/ --older-than 7d
  doze-aws data empty s3 uploads --old-versions
  doze-aws data empty logs /aws/lambda/ingest --older-than 3d
  doze-aws data empty lambda --unused        code no function uses any more
  doze-aws data reset s3                     every bucket gone, as if S3 had never run
  doze-aws data reset                        everything: a fresh start
  doze-aws data compact                      give back the space earlier deletes left

A resource name found in one region is emptied there; when it is in several, say which with --region.

Every removal says what will go and asks first (--yes for scripts, --dry-run to only look), and ends by giving the space back to
the disk. It works with doze-aws running or not.";

/// `doze-aws data ...`.
#[derive(Debug, Args)]
pub(crate) struct DataArgs {
    #[command(subcommand)]
    pub(crate) action: Option<DataAction>,
    /// A service (s3, sqs, logs...): its resources.
    pub(crate) service: Option<String>,
    /// One of its resources: what is in it.
    pub(crate) resource: Option<String>,
    /// The report as JSON (what `GET /_doze/data` answers).
    #[arg(long)]
    pub(crate) json: bool,
}

/// `doze-aws data <action>`.
#[derive(Debug, Subcommand)]
pub(crate) enum DataAction {
    /// Remove a resource's data and keep the resource (its settings, policies and triggers stay).
    Empty(EmptyArgs),
    /// Remove everything doze-aws keeps, or one service's: resources too, as if it had never run.
    Reset(ResetArgs),
    /// Give back to the disk the space that deletes left inside doze-aws's files.
    Compact,
}

/// `doze-aws data empty`.
#[derive(Debug, Args)]
pub(crate) struct EmptyArgs {
    /// The service: sqs, s3, dynamodb, kinesis, logs, lambda, stepfunctions, cloudwatch, eventbridge...
    pub(crate) service: String,
    /// The resource: a queue, bucket, table, stream, log group... (not needed with --unused).
    pub(crate) name: Option<String>,
    /// Only what is older than this: 90s, 30m, 12h, 7d, 2w.
    #[arg(long, value_name = "AGE")]
    pub(crate) older_than: Option<String>,
    /// Only keys under this prefix (S3).
    #[arg(long)]
    pub(crate) prefix: Option<String>,
    /// Only old object versions and delete markers; current objects stay (S3).
    #[arg(long)]
    pub(crate) old_versions: bool,
    /// Only what nothing uses: Lambda code no function or version points at; an S3 bucket's unfinished multipart uploads.
    #[arg(long)]
    pub(crate) unused: bool,
    /// Say what would go, and remove nothing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Do not ask.
    #[arg(long, short = 'y')]
    pub(crate) yes: bool,
}

/// `doze-aws data reset`.
#[derive(Debug, Args)]
pub(crate) struct ResetArgs {
    /// Only this service. Default: every service.
    pub(crate) service: Option<String>,
    /// Say what would go, and remove nothing.
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Do not ask.
    #[arg(long, short = 'y')]
    pub(crate) yes: bool,
}

/// `doze-aws config ...`.
#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print the effective configuration as commented TOML (a valid doze-aws.toml), with where each value came from.
    Print,
}

/// `doze-aws env`.
#[derive(Debug, Args)]
pub(crate) struct EnvArgs {
    /// The shell to write for: sh (also bash and zsh), fish or powershell. Default: from $SHELL.
    #[arg(long, value_parser = ["sh", "bash", "zsh", "fish", "powershell", "pwsh"])]
    pub(crate) shell: Option<String>,
    /// Print the lines that undo it instead.
    #[arg(long)]
    pub(crate) unset: bool,
}

/// `doze-aws apply`.
#[derive(Debug, Args)]
pub(crate) struct ApplyArgs {
    /// The template. Default: ./template.yaml, ./template.yml or ./template.json.
    #[arg(id = "template-file", value_name = "TEMPLATE")]
    pub(crate) template: Option<PathBuf>,
    /// The stack's name. Default: the working directory's name.
    #[arg(long)]
    pub(crate) stack: Option<String>,
    /// A template parameter (repeatable). Parameters not given keep the stack's current values.
    #[arg(long = "param", value_name = "KEY=VALUE")]
    pub(crate) params: Vec<String>,
}

/// `doze-aws export`.
#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    /// The stack. Default: the working directory's name.
    #[arg(long)]
    pub(crate) stack: Option<String>,
    /// The template after its transforms (SAM, includes) were applied, instead of as written.
    #[arg(long)]
    pub(crate) processed: bool,
}

/// The command line with the server settings added, at the top and (mostly hidden) on the commands that find a server.
pub(crate) fn command() -> clap::Command {
    let shown_on_commands = ["config", "data-dir", "listen"];
    Cli::command()
        .args(setting_args(&[]))
        .mut_subcommand("env", |c| c.args(setting_args(&shown_on_commands)))
        .mut_subcommand("apply", |c| c.args(setting_args(&shown_on_commands)))
        .mut_subcommand("export", |c| c.args(setting_args(&shown_on_commands)))
        .mut_subcommand("doctor", |c| c.args(setting_args(&shown_on_commands)))
        .mut_subcommand("compat", |c| c.args(setting_args(&shown_on_commands)))
        .mut_subcommand("data", |c| {
            c.args(setting_args(&shown_on_commands))
                .mut_subcommand("empty", |e| e.args(setting_args(&shown_on_commands)))
                .mut_subcommand("reset", |e| e.args(setting_args(&shown_on_commands)))
                .mut_subcommand("compact", |e| e.args(setting_args(&shown_on_commands)))
        })
        .mut_subcommand("config", |c| {
            c.mut_subcommand("print", |p| p.args(setting_args(&[])))
        })
}

/// A flag for every setting, and `--config`. With `shown` non-empty, the others are hidden from help (they still work).
fn setting_args(shown: &[&str]) -> Vec<Arg> {
    let hide = |name: &str| !shown.is_empty() && !shown.contains(&name);
    let config = Arg::new("config")
        .long("config")
        .env("DOZE_AWS_CONFIG")
        .hide_env_values(true)
        .value_name("FILE")
        .value_parser(clap::value_parser!(PathBuf))
        .help("The config file [default: ./doze-aws.toml when there is one]")
        .hide(hide("config"));
    let mut args = vec![config];
    for key in KEYS {
        let mut arg = Arg::new(key.name)
            .long(key.name)
            .env(env_var(key.name))
            .hide_env_values(true)
            .help_heading(key.group)
            .hide(hide(key.name));
        arg = match key.kind {
            Kind::Switch => arg.action(ArgAction::SetTrue).help(key.help),
            Kind::List => arg
                .action(ArgAction::Append)
                .value_delimiter(',')
                .value_name(key.value_name)
                .help(with_default(key.help, key.default)),
            _ => arg
                .value_name(key.value_name)
                .help(with_default(key.help, key.default)),
        };
        if key.name == "account" {
            arg = arg.visible_alias("account-id");
        }
        args.push(arg);
    }
    args
}

fn with_default(help: &str, default: &str) -> String {
    if default.is_empty() {
        return help.to_owned();
    }
    format!("{help} [default: {default}]")
}

/// What the command line and the environment set, from the top level and then from the command (which wins).
pub(crate) fn given(top: &ArgMatches, sub: Option<&ArgMatches>) -> (Given, Option<PathBuf>) {
    let mut given = Given::new();
    let mut file = None;
    // The command, and the commands under it (`data empty`): a setting may be given on any of them, the innermost winning.
    let mut chain = vec![top];
    let mut next = sub;
    while let Some(m) = next {
        chain.push(m);
        next = m.subcommand().map(|(_, s)| s);
    }
    for m in chain {
        if let Some(p) = m.try_get_one::<PathBuf>("config").ok().flatten() {
            file = Some(p.clone());
        }
        for key in KEYS {
            let source = match m.value_source(key.name) {
                Some(ValueSource::CommandLine) => Source::Flag,
                Some(ValueSource::EnvVariable) => Source::Env,
                _ => continue,
            };
            let value = if key.kind == Kind::Switch {
                vec![m.get_flag(key.name).to_string()]
            } else {
                m.get_many::<String>(key.name)
                    .map(|v| v.cloned().collect())
                    .unwrap_or_default()
            };
            given.insert(key.name, (value, source));
        }
    }
    (given, file)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
