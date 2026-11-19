//! Rule 4, every error says what to do next (Beyond Go B4): each error a transcript shows, with whatever further
//! problems of the same pass are drawn under it, ends in a `fix:` line.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[test]
fn every_error_in_every_transcript_has_a_fix() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/transcripts");
    let mut missing = Vec::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let is_error =
                |l: &str| l.starts_with("| error[") || l.starts_with("| ⟨") && l.contains("error[");
            if !is_error(lines[i]) {
                i += 1;
                continue;
            }
            seen += 1;
            let start = i;
            // The group: every line of the stderr section after it.
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with('|') && !lines[j].starts_with("| !") {
                j += 1;
            }
            if !lines[start..j].iter().any(|l| l.contains("fix:")) {
                missing.push(format!(
                    "{}:{} {}",
                    path.file_name().unwrap().to_string_lossy(),
                    start + 1,
                    lines[start]
                ));
            }
            i = j;
        }
    }
    assert!(seen > 50, "the transcripts show errors ({seen})");
    assert!(
        missing.is_empty(),
        "errors with no fix:\n{}",
        missing.join("\n")
    );
}
