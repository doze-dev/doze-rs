//! `doze`: the CLI alone, for terminals, CI and servers (the desktop app carries the same CLI inside `doze-app`).

use std::process::ExitCode;

fn main() -> ExitCode {
    // `doze __daemon --config <path>`: the stack's daemon (doze-daemon's `ctl::start` runs it detached).
    if std::env::args().nth(1).as_deref() == Some(doze_daemon::ctl::ARG) {
        let args: Vec<String> = std::env::args().skip(2).collect();
        return doze_daemon::entry::main(&args, doze_cli::ctx::VERSION);
    }
    ExitCode::from(doze_cli::process::main(std::env::args_os().collect()))
}
