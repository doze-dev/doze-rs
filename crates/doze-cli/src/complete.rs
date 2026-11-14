//! Dynamic completions (proposal B5): the shell asks doze itself, as it completes, through clap_complete's
//! `COMPLETE=<shell> doze -- <words>` protocol. `doze completion <shell>` prints the small script that registers it,
//! for bash, zsh, fish and PowerShell (the four the website names).
//!
//! A name argument completes from the stack: the services doze.hcl declares (read offline, so it answers in a few
//! milliseconds whether the daemon runs or not). Each name carries its engine as its
//! description, as Go's completion did (`cache\tvalkey`). Nothing here knows a module by name.

use clap_complete::engine::CompletionCandidate;
use clap_complete::env::{Bash, EnvCompleter, Fish, Powershell, Zsh};
use std::ffi::OsString;
use std::path::Path;

/// The variable the registration script sets when it calls back into doze.
pub(crate) const VAR: &str = "COMPLETE";

/// The script that registers doze's completions with `shell`.
pub(crate) fn registration(shell: crate::cli::values::Shell) -> String {
    let mut buf = Vec::new();
    // The script calls `doze` from the PATH, so it keeps working across upgrades (and from the app's own copy).
    let _ = completer(shell).write_registration(VAR, "doze", "doze", "doze", &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Answers one completion request (`COMPLETE=<shell> doze -- doze sleep ca`): the candidates, in the shell's format.
/// `None` when `shell` is not one doze completes for.
pub(crate) fn answer(shell: &str, args: Vec<OsString>, cwd: &Path) -> Option<String> {
    let shell = shell_of(shell)?;
    let mut cmd = for_completion();
    let mut buf = Vec::new();
    completer(shell)
        .write_complete(&mut cmd, args, Some(cwd), &mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn completer(shell: crate::cli::values::Shell) -> &'static dyn EnvCompleter {
    use crate::cli::values::Shell as S;
    match shell {
        S::Bash => &Bash,
        S::Zsh => &Zsh,
        S::Fish => &Fish,
        S::Powershell => &Powershell,
    }
}

/// The services the stack here declares, each with its module (or `app`, `service`, `task`).
pub(crate) fn services() -> Vec<CompletionCandidate> {
    declared(|i| {
        Some(match i.kind() {
            doze_config::Kind::Module => i.ty.clone(),
            kind => kind.as_str().to_owned(),
        })
    })
}

/// The tasks the stack here declares.
pub(crate) fn tasks() -> Vec<CompletionCandidate> {
    declared(|i| (i.kind() == doze_config::Kind::Task).then(|| "task".to_owned()))
}

/// The instances doze.hcl declares (here, or in the nearest parent), kept by `pick`, which also gives the help.
fn declared(pick: impl Fn(&doze_config::Instance) -> Option<String>) -> Vec<CompletionCandidate> {
    let Ok(cwd) = std::env::current_dir() else {
        return Vec::new();
    };
    let Some(project) = crate::project::find(&cwd, None) else {
        return Vec::new();
    };
    let Ok(cfg) = crate::config::Loader::process(&cwd).shape(&project) else {
        return Vec::new();
    };
    cfg.instances
        .iter()
        .filter_map(|i| {
            pick(i).map(|help| CompletionCandidate::new(i.name.clone()).help(Some(help.into())))
        })
        .collect()
}

/// A run under `COMPLETE=<shell>`: the shell's call back into doze. With words after `--`, their candidates; with
/// none, the registration script (what `doze completion <shell>` prints). Exit 2 for a shell doze does not know.
pub(crate) fn run(shell: &str, argv: Vec<OsString>, ctx: &mut crate::Ctx<'_>) -> crate::Exit {
    let name = Path::new(shell)
        .file_stem()
        .map_or_else(|| shell.to_owned(), |s| s.to_string_lossy().into_owned());
    let words: Vec<OsString> = argv.into_iter().skip_while(|a| a != "--").skip(1).collect();
    if shell_of(&name).is_none() {
        let _ = writeln!(
            ctx.err,
            "doze completes for bash, zsh, fish and powershell, not {name:?}"
        );
        return crate::Exit::Usage;
    }
    let text = if words.is_empty() {
        shell_of(&name).map(registration)
    } else {
        // A request the shell framed wrongly gets no candidates, never an error in the middle of typing.
        Some(answer(&name, words, &ctx.cwd.clone()).unwrap_or_default())
    };
    match text {
        Some(t) => {
            let _ = ctx.out.write_all(t.as_bytes());
            crate::Exit::Ok
        }
        None => crate::Exit::Usage,
    }
}

fn shell_of(name: &str) -> Option<crate::cli::values::Shell> {
    SHELLS.into_iter().find(|s| completer(*s).is(name))
}

const SHELLS: [crate::cli::values::Shell; 4] = [
    crate::cli::values::Shell::Bash,
    crate::cli::values::Shell::Zsh,
    crate::cli::values::Shell::Fish,
    crate::cli::values::Shell::Powershell,
];

/// The command tree the engine completes from. clap_complete offers one name per command, the first in sorted order,
/// so a visible alias would stand in for its command (`ls` for `status`, `mod` for `modules`). Here every alias is
/// hidden: the commands' own names are offered, and an alias still completes when only it matches (`kee`).
fn for_completion() -> clap::Command {
    let full = <crate::cli::Cli as clap::CommandFactory>::command();
    let names: Vec<String> = full
        .get_subcommands()
        .map(|s| s.get_name().to_owned())
        .collect();
    names.into_iter().fold(full, |cmd, name| {
        cmd.mut_subcommand(&name, |sc| {
            let aliases: Vec<String> = sc.get_all_aliases().map(str::to_owned).collect();
            sc.alias(None).aliases(aliases)
        })
    })
}
