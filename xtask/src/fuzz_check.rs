//! Every parser has a fuzz target. A parser is marked in its source with a comment:
//!
//! ```text
//! // fuzz:parser fuzz_frame
//! ```
//!
//! and the target `crates/<crate>/fuzz/fuzz_targets/fuzz_frame.rs` must exist. A marker without a target
//! fails the build, so a new parser cannot land unfuzzed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MARKER: &str = "// fuzz:parser ";

/// A marked parser: where it is and which target is to cover it.
#[derive(Debug, PartialEq, Eq)]
struct Parser {
    source: PathBuf,
    target: String,
}

pub(crate) fn run() -> ExitCode {
    let mut parsers = Vec::new();
    if let Err(e) = scan(Path::new("crates"), &mut parsers) {
        eprintln!("xtask fuzz-check: {e}");
        return ExitCode::FAILURE;
    }
    let missing: Vec<_> = parsers.iter().filter(|p| !has_target(p)).collect();
    for p in &missing {
        println!(
            "FAIL  {}: no fuzz target `{}`",
            p.source.display(),
            p.target
        );
    }
    println!(
        "fuzz-check: {} parsers, {} without a target",
        parsers.len(),
        missing.len()
    );
    if missing.is_empty() && !parsers.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The crate a source file belongs to: `crates/<name>`.
fn crate_dir(source: &Path) -> Option<PathBuf> {
    let mut parts = source.components();
    let first = parts.next()?;
    let name = parts.next()?;
    Some(PathBuf::from(first.as_os_str()).join(name.as_os_str()))
}

fn has_target(p: &Parser) -> bool {
    crate_dir(&p.source).is_some_and(|c| {
        c.join("fuzz/fuzz_targets")
            .join(format!("{}.rs", p.target))
            .is_file()
    })
}

fn markers(source: &Path, text: &str) -> Vec<Parser> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(MARKER))
        .map(|t| Parser {
            source: source.to_path_buf(),
            target: t.trim().to_owned(),
        })
        .collect()
}

fn scan(dir: &Path, out: &mut Vec<Parser>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if !matches!(name, "target" | "corpus" | "artifacts" | "fuzz") {
                scan(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.extend(markers(&path, &fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_markers() {
        let src = "fn a() {}\n    // fuzz:parser fuzz_frame\nfn b() {}\n// fuzz:parser  fuzz_x \n";
        let got = markers(Path::new("crates/kafka-wire/src/frame.rs"), src);
        assert_eq!(
            got.iter().map(|p| p.target.as_str()).collect::<Vec<_>>(),
            ["fuzz_frame", "fuzz_x"]
        );
    }

    #[test]
    fn a_target_belongs_to_the_crate_of_its_parser() {
        let p = Parser {
            source: PathBuf::from("crates/kafka-wire/src/frame.rs"),
            target: "fuzz_frame".into(),
        };
        assert_eq!(
            crate_dir(&p.source),
            Some(PathBuf::from("crates/kafka-wire"))
        );
    }
}
