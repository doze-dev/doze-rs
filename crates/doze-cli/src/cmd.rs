//! One file per command that is built; the rest answer from [`stub`]. Dispatch is one exhaustive match, so a command
//! added to the surface cannot go unanswered.

mod ask;
pub(crate) mod declared;
pub(crate) mod docs;
mod docs_view;
pub(crate) mod doctor;
mod dry;
pub(crate) mod fetch;
pub(crate) mod format;
mod graph;
pub(crate) mod lint;
pub(crate) mod logs;
pub(crate) mod modules;
mod pin;
mod ready;
pub(crate) mod report;
pub(crate) mod setup;
pub(crate) mod status;
pub(crate) mod stub;
pub(crate) mod task;
mod trust;
pub(crate) mod update;
mod verbs;
pub(crate) mod verify;
pub(crate) mod version;
mod watch;
pub(crate) mod why;

use crate::cli::Cmd;
use crate::cli::setup::ModulesCmd;
use crate::ctx::Ctx;
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::view::View;

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
        Cmd::Completion(c) => Ok(Done::ok(View::Raw(crate::complete::registration(c.shell)))),
        Cmd::Doctor(d) if d.dry_run && !d.fix && !d.print_fix => Err(crate::diag::Diagnostic::new(
            crate::diag::ids::USAGE,
            "--dry-run goes with --fix: doctor itself changes nothing",
        )
        .fix("doze doctor --fix --dry-run")),
        Cmd::Doctor(d) if d.fix || d.print_fix => setup::fix(ctx, mode, d.dry_run || d.print_fix),
        Cmd::Doctor(_) => doctor::run(ctx, mode),
        Cmd::DnsSetup(a) => setup::dns_setup(ctx, &a),
        Cmd::Trust(a) => trust::run_trust(ctx, mode, &a),
        Cmd::Update(u) => update::run(ctx, u.dry_run),
        Cmd::Refresh(r) => update::refresh(ctx, &r),
        Cmd::Status(s) => status::run(ctx, mode, &s, notices),
        Cmd::Wake(n) => verbs::wake(ctx, mode, &n),
        Cmd::Sleep(n) => verbs::sleep(ctx, mode, &n),
        Cmd::Restart(n) => verbs::restart(ctx, mode, &n),
        Cmd::Reset(n) => verbs::reset(ctx, mode, &n),
        Cmd::Off(n) => verbs::on_off(ctx, mode, &n, true),
        Cmd::On(n) => verbs::on_off(ctx, mode, &n, false),
        Cmd::Pin(p) => pin::run(ctx, mode, &p),
        Cmd::Task(t) => task::run(ctx, mode, &t, notices),
        Cmd::Logs(l) => logs::run(ctx, mode, &l),
        Cmd::Why(w) => why::run(ctx, mode, &w),
        Cmd::Lint(l) => lint::run(ctx, mode, &l),
        Cmd::Fmt(f) => format::run(ctx, mode, &f),
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
