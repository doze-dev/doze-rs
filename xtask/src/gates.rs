//! `cargo xtask gates [--quick] [--keep-going]`: the checks that run before a commit, in the order that
//! fails fastest: format, lint, the file-size and fuzz-target gates, then the tests. Prints one line per
//! step with its time. `--quick` skips the tests; `--keep-going` runs every step even after a failure.
//!
//! One cargo at a time and few jobs: a loaded machine gave false failures before (see the PLAN log).
//! `GATES_JOBS` sets the job count (default 4). Run it under `nice -n 19`.

use std::process::{Command, ExitCode};
use std::time::Instant;

struct Step {
    name: &'static str,
    program: &'static str,
    args: Vec<String>,
    /// Skipped by `--quick`.
    slow: bool,
}

pub(crate) fn run(args: &[String]) -> ExitCode {
    let (mut quick, mut keep_going) = (false, false);
    for a in args {
        match a.as_str() {
            "--quick" => quick = true,
            "--keep-going" => keep_going = true,
            other => {
                eprintln!(
                    "xtask gates: unknown argument {other}\nusage: gates [--quick] [--keep-going]"
                );
                return ExitCode::from(2);
            }
        }
    }
    let jobs = std::env::var("GATES_JOBS")
        .ok()
        .and_then(|j| j.parse::<u32>().ok())
        .unwrap_or(4);
    let mut failed = Vec::new();
    for step in steps(jobs).into_iter().filter(|s| !(quick && s.slow)) {
        let started = Instant::now();
        let ok = Command::new(step.program)
            .args(&step.args)
            .status()
            .is_ok_and(|s| s.success());
        println!(
            "gate {:<12} {:<5} {:>6.1}s",
            step.name,
            if ok { "ok" } else { "FAIL" },
            started.elapsed().as_secs_f32()
        );
        if !ok {
            failed.push(step.name);
            if !keep_going {
                break;
            }
        }
    }
    summary(&failed)
}

fn summary(failed: &[&str]) -> ExitCode {
    if failed.is_empty() {
        println!("gates: all passed");
        ExitCode::SUCCESS
    } else {
        println!("gates: failed: {}", failed.join(", "));
        ExitCode::FAILURE
    }
}

fn steps(jobs: u32) -> Vec<Step> {
    let j = jobs.to_string();
    let step = |name, program, args: &[&str], slow| Step {
        name,
        program,
        args: args.iter().map(ToString::to_string).collect(),
        slow,
    };
    vec![
        step("fmt", "cargo", &["fmt", "--all", "--check"], false),
        step(
            "clippy",
            "cargo",
            &[
                "clippy",
                "-j",
                &j,
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            false,
        ),
        step("lint-size", "cargo", &["xtask", "lint-size"], false),
        step("fuzz-check", "cargo", &["xtask", "fuzz-check"], false),
        step(
            "test",
            "cargo",
            &[
                "test",
                "-j",
                &j,
                "--workspace",
                "--",
                &format!("--test-threads={jobs}"),
            ],
            true,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cheap_checks_come_before_the_tests() {
        let all = steps(4);
        let names: Vec<_> = all.iter().map(|s| s.name).collect();
        assert_eq!(names, ["fmt", "clippy", "lint-size", "fuzz-check", "test"]);
        assert!(all.iter().filter(|s| s.slow).all(|s| s.name == "test"));
    }

    #[test]
    fn the_job_count_reaches_cargo_and_the_test_harness() {
        let all = steps(3);
        let test = all.iter().find(|s| s.name == "test").unwrap();
        assert!(test.args.contains(&"--test-threads=3".to_string()));
        assert!(test.args.windows(2).any(|w| w == ["-j", "3"]));
    }

    #[test]
    fn a_failure_names_the_step() {
        assert_eq!(summary(&[]), ExitCode::SUCCESS);
        assert_eq!(summary(&["fmt"]), ExitCode::FAILURE);
    }
}
