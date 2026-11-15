//! `doze`: the CLI alone, for terminals, CI and servers (the desktop app carries the same CLI inside `doze-app`).

use std::process::ExitCode;

fn main() -> ExitCode {
    // `doze __daemon --config <path>`: the stack's daemon (doze-daemon's `ctl::start` runs it detached).
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == doze_daemon::ctl::ARG)
    {
        let args: Vec<String> = std::env::args_os()
            .skip(2)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        return doze_daemon::entry::main(&args, doze_cli::ctx::VERSION);
    }
    ExitCode::from(doze_cli::process::main(std::env::args_os().collect()))
}
