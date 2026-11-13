//! Repository tasks. Run as `cargo xtask <task>`.
//!
//! - `fuzz-check`: every parser marked `// fuzz:parser <target>` has that fuzz target.
//! - `lint-size`: no `.rs` file may grow past the hard line limit (and files past
//!   the soft limit are listed, so they get split before they hurt).
//! - `weight`: release size, package count and idle memory against `xtask/weight-limits.txt`.
//! - `conformance`: the boto3 harness against a server, scored against the AWS recordings.
//! - `codegen`: regenerates the `aws-model-*` crates from the models (`--check` fails if they are stale).
//! - `themes`: converts the Go app's design tokens into the desktop app's themes (`--check` fails if they are stale).
//! - `gates`: the pre-commit checks, in order.

mod codegen;
mod conformance;
mod fuzz_check;
mod gates;
mod lint_size;
mod themes;
mod weight;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("lint-size") => lint_size::run(args.collect()),
        Some("fuzz-check") => fuzz_check::run(),
        Some("weight") => weight::run(&args.collect::<Vec<_>>()),
        Some("codegen") => codegen::run(&args.collect::<Vec<_>>()),
        Some("conformance") => conformance::run(args.collect()),
        Some("themes") => themes::run(&args.collect::<Vec<_>>()),
        Some("gates") => gates::run(&args.collect::<Vec<_>>()),
        Some(other) => usage(&format!("unknown task `{other}`")),
        None => usage("no task given"),
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!(
        "xtask: {problem}\n\ntasks:\n  \
         lint-size [--soft N] [--hard N]   file-size gate\n  \
         fuzz-check                        every marked parser has a fuzz target\n  \
         weight [package ...]              binary size, packages, idle memory against the limits file\n  \
         conformance [--cmd T | --endpoint U] [module ...]   score a server against the AWS recordings\n  \
         codegen [--check]                 regenerate the aws-model crates (or fail if they are stale)\n  \
         themes [--check]                  the desktop app's themes from tokens.css (or fail if stale)
         gates [--quick] [--keep-going]    fmt, clippy, lint-size, fuzz-check, tests"
    );
    ExitCode::from(2)
}
