//! What a page says is said in plain words: no source file name and no crate path reaches the user. A descriptor's titles,
//! sentences and confirmations are read off its debug form; the sentences the kinds' `says` build, and every other string
//! under `resources/`, are read from the source text, comments left out.

use std::fs;
use std::path::{Path, PathBuf};

use super::ALL;

/// What a source path looks like in a sentence. Built in two pieces so this file does not hold it whole.
fn cites(text: &str) -> bool {
    let rs = [".", "rs"].concat();
    let krate = ["crates", "/"].concat();
    let after_rs = text.match_indices(&rs).any(|(i, _)| {
        !text[i + rs.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    });
    after_rs || text.contains(&krate)
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("the resources directory is readable") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && path.file_name() != Some("plain_words.rs".as_ref())
        {
            out.push(path);
        }
    }
}

/// The text inside the double quotes of one line of Rust, which is not a comment.
fn quoted(line: &str) -> Vec<&str> {
    let code = line.trim_start();
    if code.starts_with("//") {
        return Vec::new();
    }
    code.split('"').skip(1).step_by(2).collect()
}

#[test]
fn no_descriptor_names_a_source_file() {
    for d in ALL.iter().filter_map(|e| e.descriptor) {
        let dump = format!("{d:?}");
        let strings: Vec<&str> = dump.split('"').skip(1).step_by(2).collect();
        assert!(!strings.is_empty(), "{}: nothing was read", d.service);
        for s in strings {
            assert!(
                !cites(s),
                "{}: a descriptor string cites source: {s}",
                d.service
            );
        }
    }
}

#[test]
fn no_string_under_resources_names_a_source_file() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/resources"),
        &mut files,
    );
    assert!(files.len() > 40, "only {} files were read", files.len());
    for file in files {
        let text = fs::read_to_string(&file).expect("a source file");
        for (n, line) in text.lines().enumerate() {
            for s in quoted(line) {
                assert!(
                    !cites(s),
                    "{}:{}: a string cites source: {s}",
                    file.display(),
                    n + 1
                );
            }
        }
    }
}

#[test]
fn the_check_tells_a_citation_from_a_word() {
    assert!(cites("see retry.rs for it"));
    assert!(cites("crates/aws-sqs"));
    assert!(!cites("the .rsvp list, or users.rsa"));
    assert!(quoted("    // \"a.rs\" in a comment").is_empty());
    assert_eq!(quoted("let a = \"x\"; let b = \"y\";"), vec!["x", "y"]);
}
