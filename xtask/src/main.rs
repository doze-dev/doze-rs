//! Repository tasks. Run as `cargo xtask <task>`.
//!
//! - `fuzz-check`: every parser marked `// fuzz:parser <target>` has that fuzz target.
//! - `lint-size`: no `.rs` file may grow past the hard line limit (and files past
//!   the soft limit are listed, so they get split before they hurt).

mod fuzz_check;
mod lint_size;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("lint-size") => lint_size::run(args.collect()),
        Some("fuzz-check") => fuzz_check::run(),
        Some(other) => usage(&format!("unknown task `{other}`")),
        None => usage("no task given"),
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!(
        "xtask: {problem}\n\ntasks:\n  lint-size [--soft N] [--hard N]   file-size gate\n  fuzz-check                        every marked parser has a fuzz target"
    );
    ExitCode::from(2)
}
