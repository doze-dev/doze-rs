//! Repository tasks. Run as `cargo xtask <task>`.
//!
//! - `lint-size`: no `.rs` file may grow past the hard line limit (and files past
//!   the soft limit are listed, so they get split before they hurt).

mod lint_size;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("lint-size") => lint_size::run(args.collect()),
        Some(other) => usage(&format!("unknown task `{other}`")),
        None => usage("no task given"),
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!("xtask: {problem}\n\ntasks:\n  lint-size [--soft N] [--hard N]   file-size gate");
    ExitCode::from(2)
}
