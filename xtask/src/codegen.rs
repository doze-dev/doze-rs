//! `cargo xtask codegen [--check]`: regenerates the `aws-model-*` crates from `models/` and the overlays in
//! `codegen/overlays/`. With `--check` nothing is written: the task fails if any generated file differs from
//! what the models would produce (PLAN.md rule 11).

use std::process::{Command, ExitCode};

pub(crate) fn run(args: &[String]) -> ExitCode {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "-p", "codegen", "--"]);
    cmd.args([
        "--models",
        "models",
        "--overlays",
        "codegen/overlays",
        "--out",
        "crates",
    ]);
    cmd.args(args);
    match cmd.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("xtask codegen: cannot run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}
