//! `doze`: the CLI alone, for terminals, CI and servers (the desktop app carries the same CLI inside `doze-app`).

use std::process::ExitCode;

fn main() -> ExitCode {
    // `process::main` owns every word, the hidden ones too (`__daemon`, `__fetch`): a program that embeds the CLI
    // hosts its own daemon the same way.
    ExitCode::from(doze_cli::process::main(std::env::args_os().collect()))
}
