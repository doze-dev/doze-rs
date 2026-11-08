//! The file-size gate: every `.rs` file stays under a hard line limit; files over a
//! soft limit are listed so they are split by responsibility before they hit it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SOFT: usize = 300;
const HARD: usize = 450;

/// A file and its length in lines.
#[derive(Debug, PartialEq, Eq)]
struct Sized {
    path: PathBuf,
    lines: usize,
}

pub(crate) fn run(args: Vec<String>) -> ExitCode {
    let Some((soft, hard)) = parse_limits(&args) else {
        eprintln!("xtask lint-size: usage: lint-size [--soft N] [--hard N]");
        return ExitCode::from(2);
    };
    let root = Path::new(".");
    let mut files = Vec::new();
    for dir in ["crates", "xtask"] {
        if let Err(e) = collect(&root.join(dir), &mut files) {
            eprintln!("xtask lint-size: reading {dir}: {e}");
            return ExitCode::FAILURE;
        }
    }
    files.sort_by(|a, b| b.lines.cmp(&a.lines).then_with(|| a.path.cmp(&b.path)));
    report(&files, soft, hard)
}

fn parse_limits(args: &[String]) -> Option<(usize, usize)> {
    let (mut soft, mut hard) = (SOFT, HARD);
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next()?.parse().ok()?;
        match flag.as_str() {
            "--soft" => soft = value,
            "--hard" => hard = value,
            _ => return None,
        }
    }
    (soft <= hard).then_some((soft, hard))
}

fn report(files: &[Sized], soft: usize, hard: usize) -> ExitCode {
    let over_hard: Vec<_> = files.iter().filter(|f| f.lines > hard).collect();
    let over_soft: Vec<_> = files
        .iter()
        .filter(|f| f.lines > soft && f.lines <= hard)
        .collect();
    for f in &over_soft {
        println!(
            "warn  {:>4} lines  {}  (over the soft limit of {soft}: split it soon)",
            f.lines,
            f.path.display()
        );
    }
    for f in &over_hard {
        println!(
            "FAIL  {:>4} lines  {}  (over the hard limit of {hard})",
            f.lines,
            f.path.display()
        );
    }
    println!(
        "lint-size: {} files, {} over soft, {} over hard",
        files.len(),
        over_soft.len(),
        over_hard.len()
    );
    if over_hard.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Collects every `.rs` file under `dir`, skipping build output and fuzz corpora.
fn collect(dir: &Path, out: &mut Vec<Sized>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            let skip = matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("target" | "corpus" | "artifacts")
            );
            if !skip {
                collect(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            let lines = fs::read_to_string(&path)?.lines().count();
            out.push(Sized { path, lines });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(lines: usize) -> Sized {
        Sized {
            path: PathBuf::from("a.rs"),
            lines,
        }
    }

    #[test]
    fn limits_default_and_override() {
        assert_eq!(parse_limits(&[]), Some((SOFT, HARD)));
        let args = ["--soft", "10", "--hard", "20"].map(String::from);
        assert_eq!(parse_limits(&args), Some((10, 20)));
    }

    #[test]
    fn bad_limits_are_refused() {
        for args in [
            &["--soft"][..],
            &["--soft", "x"],
            &["--nope", "1"],
            &["--soft", "9", "--hard", "3"],
        ] {
            let args: Vec<String> = args.iter().map(ToString::to_string).collect();
            assert_eq!(parse_limits(&args), None, "{args:?}");
        }
    }

    #[test]
    fn only_the_hard_limit_fails() {
        assert_eq!(report(&[file(100), file(400)], 300, 450), ExitCode::SUCCESS);
        assert_eq!(report(&[file(451)], 300, 450), ExitCode::FAILURE);
        assert_eq!(report(&[file(450)], 300, 450), ExitCode::SUCCESS);
    }
}
