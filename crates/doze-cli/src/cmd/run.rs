//! `doze run -- <command> [args…]` (Go cmd/doze/run.go): the daemon is started if it is not running (as `up` starts
//! it, under the stack lock), so the stack's services wake on the command's first connection; then the command runs
//! on this terminal and doze exits with its code. Nothing is set in its environment: every instance has a fixed
//! address. `--env` (the variables `doze env` prints) waits for each module's own answer on the wire (D1's module
//! catalog) and says so.

use crate::cli::args::Run;
use crate::cmd::{stub, up};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::Stack;
use crate::view::View;
use std::path::Path;

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Run) -> Outcome {
    if a.env {
        return Err(stub::not_built("doze run --env", stub::Needs::Daemon).note(
            "the variables come from each module's own answer, which the Rust daemon does not give yet; doze run without --env works",
        ));
    }
    let stack = Stack::find(ctx, mode)?;
    stack.loader.shape(&stack.project)?; // Go reads doze.hcl first: a mistake in it is exit 5 before anything starts
    if stack.open(mode.timeout)?.is_none() {
        let _lock = crate::stack::lock(ctx, &stack)?; // held while it starts, not while the command runs
        up::start(ctx, mode, &stack)?;
    }
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
    match ctx.host.run(program, &args) {
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
