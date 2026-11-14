//! `doze`: the CLI alone, for terminals, CI and servers (the desktop app carries the same CLI inside `doze-app`).

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(doze_cli::process::main(std::env::args_os().collect()))
}
