//! No oracle fixture may carry a path from the machine it was made on: every `testdata` folder in
//! the workspace is searched for a home folder or a macOS temporary folder.

use std::path::{Path, PathBuf};

const FORBIDDEN: [&str; 3] = ["/Users/", "/private/tmp", "/var/folders"];
/// A home no machine has: allowed.
const PLACEHOLDER: &str = "/Users/dev/";

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn no_testdata_holds_a_local_path() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut all = Vec::new();
    for c in std::fs::read_dir(&crates).unwrap().flatten() {
        let td = c.path().join("testdata");
        if td.is_dir() {
            files(&td, &mut all);
        }
    }
    assert!(!all.is_empty(), "no testdata found under {crates:?}");
    let mut bad = Vec::new();
    for f in all {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for needle in FORBIDDEN {
            // `/Users/dev/` is a made-up home in hand-written fixtures (a stack trace in doze-control's scripts).
            let real = text
                .match_indices(needle)
                .any(|(at, _)| !text[at..].starts_with(PLACEHOLDER));
            if real {
                bad.push(format!("{} holds {needle}", f.display()));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "local paths in testdata:\n{}",
        bad.join("\n")
    );
}
