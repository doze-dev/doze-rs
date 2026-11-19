//! `doze run -- <command> [args…]` (Go cmd/doze/run.go): the daemon is started if it is not running (as `up` starts
//! it, under the stack lock), so the stack's services wake on the command's first connection; then the command runs
//! on this terminal and doze exits with its code. Nothing is set in its environment: every instance has a fixed
//! address. `--env` adds the variables `doze env` prints (each module's own answer on the wire).

use crate::cli::args::Run;
use crate::cmd::up;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::Stack;
use crate::view::View;
use std::path::Path;

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Run) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let cfg = stack.loader.shape(&stack.project)?; // Go reads doze.hcl first: a mistake in it is exit 5 before anything starts
    let conn = match stack.open(mode.timeout)? {
        Some(c) => c,
        None => {
            let _lock = crate::stack::lock(ctx, &stack)?; // held while it starts, not while the command runs
            up::start(ctx, mode, &stack)?
        }
    };
    let env: Vec<(String, String)> = if a.env {
        let snap = crate::stack::snapshot(&conn)?;
        crate::cmd::env::pairs(ctx, &cfg, &snap, &[])
            .into_iter()
            .map(|p| (p.var, p.value))
            .collect()
    } else {
        Vec::new()
    };
    drop(conn);
    let Some((program, args)) = a.command.split_first() else {
        return Err(
            Diagnostic::new(ids::USAGE, "doze run needs a command").fix("doze run -- <command>")
        );
    };
    let program = Path::new(program);
    let args: Vec<String> = args
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let _ = ctx.out.flush();
    match ctx.host.run_env(program, &args, &env) {
        Ok(0) => Ok(Done::ok(View::Silent)),
        Ok(code) => Err(Diagnostic::new(
            ids::command_exit(code),
            format!("{} exited with status {code}", program.display()),
        )),
        Err(e) => Err(Diagnostic::new(
            ids::COMMAND_UNRUNNABLE,
            format!("could not run {}: {e}", program.display()),
        )
        .fix("check the command after --; doze run -- <command> [args…]")),
    }
}
