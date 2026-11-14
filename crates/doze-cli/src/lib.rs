//! The doze command (docs/port/cli/proposal.md). The terminal is a client of doze, as the app is: the commands that
//! talk to the daemon arrive with the Rust core; until then they say so (`cmd/stub.rs`). What works without the core
//! works now: `version`, `completion`, `modules`, `modules docs`, `modules verify`, `doctor`, `update --dry-run`.
//!
//! A run is [`run`] over a [`Ctx`]: everything it touches outside itself is passed in, so the tests run it in-process
//! with buffers, a temporary doze home and a registry on disk, and `main` runs it on the real process.

pub mod cli;
pub mod clock;
mod cmd;
mod complete;
mod config;
pub mod ctx;
pub mod diag;
mod diff;
pub mod exit;
pub mod help;
pub mod install;
pub mod lock;
mod machine;
pub mod notice;
pub mod output;
pub(crate) mod paint;
pub mod plan;
pub mod platform;
pub mod process;
pub mod project;
pub mod registry;
mod stack;
mod stream;
mod suggest;
mod table;
pub mod update;
pub mod view;

use clap::{ArgMatches, CommandFactory, FromArgMatches};
use cli::{Cli, Cmd};
pub use ctx::{Ctx, Host, Terminals};
use diag::{Diagnostic, ids};
pub use exit::Exit;
use notice::Notices;
use output::Mode;
use std::ffi::OsString;

// fuzz:parser fuzz_argv
/// Runs one command line (`argv[0]` is the program) and says how it ended.
pub fn run(argv: Vec<OsString>, ctx: &mut Ctx<'_>) -> Exit {
    // A shell asking for completions (`COMPLETE=<shell> doze -- …`): answer it and nothing else.
    if let Some(shell) = ctx.var(complete::VAR).filter(|v| v != "0") {
        return complete::run(&shell, argv, ctx);
    }
    let asked_json = argv
        .iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--json");
    let root = Cli::command();
    let help = help::root(&root);
    let mut root = root.override_help(help);
    let typed = typed_path(&root, &argv);
    let parsed = root
        .try_get_matches_from_mut(argv)
        .and_then(|m| Cli::from_arg_matches(&m).map(|cli| (command_path(&m), cli)));
    let (path, cli) = match parsed {
        Ok(p) => p,
        Err(e) => return clap_error(ctx, &e, &typed, asked_json),
    };
    let notices = Notices::default();
    let mode = match Mode::new(&cli.global, cli.command.as_ref(), ctx) {
        Ok(m) => m,
        Err(d) => {
            return output::finish(
                ctx,
                &Mode::fallback(ctx, asked_json),
                &path,
                Err(d),
                &notices,
            );
        }
    };
    let tell = !matches!(
        cli.command,
        Some(Cmd::Update(_) | Cmd::Completion(_) | Cmd::Refresh(_))
    );
    let mut notices = notices;
    let outcome = cmd::dispatch(cli.command, &path, ctx, &mode, &mut notices);
    let exit = output::finish(ctx, &mode, &path, outcome, &notices);
    if let Some(line) = update::notice(ctx, &mode, tell) {
        let _ = writeln!(ctx.err, "{}", mode.err().dim(&line));
    }
    exit
}

/// `doze modules docs`, from the parsed command line (aliases come back as the command's own name).
fn command_path(m: &ArgMatches) -> String {
    let mut path = String::from("doze");
    let mut cur = m;
    while let Some((name, sub)) = cur.subcommand() {
        path.push(' ');
        path.push_str(name);
        cur = sub;
    }
    path
}

/// Help asked for prints it (exit 0); anything else clap refuses is a usage error (exit 2) in doze's own shape.
/// The commands named on a command line, as far as they are commands (`doze modules docs` for
/// `doze modules docs --wbe valkey`), for "see its --help" when the line does not parse.
fn typed_path(root: &clap::Command, argv: &[OsString]) -> String {
    let mut path = String::from("doze");
    let mut cur = root;
    for word in argv
        .iter()
        .skip(1)
        .filter_map(|a| a.to_str())
        .filter(|a| !a.starts_with('-'))
    {
        let Some(sub) = cur.find_subcommand(word) else {
            break;
        };
        path.push(' ');
        path.push_str(sub.get_name());
        cur = sub;
    }
    path
}

/// Help asked for prints it (exit 0); anything else clap refuses is a usage error (exit 2) in doze's own shape: clap's
/// first paragraph is the message, its tips are notes, and the fix is the help of the command that was typed.
fn clap_error(ctx: &mut Ctx<'_>, e: &clap::Error, typed: &str, json: bool) -> Exit {
    use clap::error::ErrorKind;
    let text = e.render().to_string();
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        let _ = write!(ctx.out, "{text}");
        return Exit::Ok;
    }
    let mut paragraphs = text
        .split("\n\n")
        .map(|p| p.lines().map(str::trim).collect::<Vec<_>>().join(" "));
    let first = paragraphs.next().unwrap_or_default();
    let mut d = Diagnostic::new(
        ids::USAGE,
        first
            .strip_prefix("error: ")
            .unwrap_or(&first)
            .trim()
            .to_string(),
    );
    for p in paragraphs {
        if let Some(tip) = p.strip_prefix("tip: ") {
            d = d.note(tip.to_string());
        }
    }
    d = d.fix(format!("{typed} --help"));
    output::finish(
        ctx,
        &Mode::fallback(ctx, json),
        typed,
        Err(d),
        &Notices::default(),
    )
}

/// Whether `word` names a doze command (or one of its aliases, hidden ones included): what a program that carries the
/// CLI beside its own face (the desktop app) checks to hand a command line to [`process::main`].
pub fn is_command(word: &str) -> bool {
    word == "help"
        || word == doze_names::HELPER_ARG
        || Cli::command().find_subcommand(word).is_some()
}
