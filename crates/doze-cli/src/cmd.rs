//! One file per command that is built; the rest answer from [`stub`]. Dispatch is one exhaustive match, so a command
//! added to the surface cannot go unanswered.

pub(crate) mod docs;
mod docs_view;
pub(crate) mod doctor;
pub(crate) mod fetch;
pub(crate) mod modules;
pub(crate) mod stub;
pub(crate) mod update;
pub(crate) mod verify;
pub(crate) mod version;

use crate::cli::setup::ModulesCmd;
use crate::cli::{Cli, Cmd};
use crate::ctx::Ctx;
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::view::View;
use clap::CommandFactory;

/// Runs `cmd` (`command` is its path, `doze modules docs`, for messages).
pub(crate) fn dispatch(
    cmd: Option<Cmd>,
    command: &str,
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    notices: &mut Notices,
) -> Outcome {
    let Some(cmd) = cmd else {
        return Err(stub::bare());
    };
    let command = match &cmd {
        Cmd::Doctor(d) if d.fix || d.print_fix => "doze doctor --fix",
        _ => command,
    };
    if let Some(needs) = stub::needs(&cmd) {
        return Err(stub::not_built(command, needs));
    }
    match cmd {
        Cmd::Version => version::run(),
        Cmd::Completion(c) => Ok(Done::ok(View::Raw(completion(c.shell)))),
        Cmd::Doctor(_) => doctor::run(ctx, mode),
        Cmd::Update(u) => update::run(ctx, u.dry_run),
        Cmd::Refresh(r) => update::refresh(ctx, &r),
        Cmd::Modules(m) => match m.command {
            None => modules::run(ctx, mode),
            Some(ModulesCmd::Docs(d)) => docs::run(ctx, mode, &d, notices),
            Some(ModulesCmd::Verify) => verify::run(ctx, mode),
            Some(ModulesCmd::Upgrade(_)) => Err(stub::not_built(command, stub::Needs::Lock)),
        },
        other => Err(stub::not_built(
            command,
            stub::needs(&other).unwrap_or(stub::Needs::Daemon),
        )),
    }
}

/// The completion script for `shell`: commands, aliases and flags. Service and module names join when the core can
/// list them (the proposal's dynamic completions).
fn completion(shell: crate::cli::values::Shell) -> String {
    use crate::cli::values::Shell as S;
    use clap_complete::Shell as C;
    let shell = match shell {
        S::Bash => C::Bash,
        S::Zsh => C::Zsh,
        S::Fish => C::Fish,
        S::Powershell => C::PowerShell,
    };
    // clap_complete offers hidden commands too (bash and zsh list `__refresh`): complete a tree without them.
    let full = Cli::command();
    let mut shown = clap::Command::new("doze")
        .args(full.get_arguments().cloned())
        .subcommands(full.get_subcommands().filter(|s| !s.is_hide_set()).cloned());
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut shown, "doze", &mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}
