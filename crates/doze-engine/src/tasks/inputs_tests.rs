#![allow(clippy::unwrap_used)]

use super::*;

fn write(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn pats(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| (*s).to_string()).collect()
}

/// Go's `TestMatch`, and `path.Match`'s classes and escapes as Go answers them (asked of Go's `inputs.Match`).
#[test]
fn match_is_gos() {
    let cases = [
        ("migrations/*.sql", "migrations/001.sql", true),
        ("migrations/*.sql", "migrations/sub/001.sql", false),
        ("migrations/*.sql", "other/001.sql", false),
        ("**/*.go", "main.go", true),
        ("**/*.go", "cmd/api/main.go", true),
        ("**/*.go", "cmd/api/main.js", false),
        ("cmd/**", "cmd/api/main.go", true),
        ("cmd/**", "cmd", true),
        ("cmd/**/main.go", "cmd/main.go", true),
        ("cmd/**/main.go", "cmd/a/b/main.go", true),
        ("go.mod", "go.mod", true),
        ("go.mod", "sub/go.mod", false),
        ("file?.txt", "file1.txt", true),
        ("file?.txt", "file10.txt", false),
        ("[a-c]x", "bx", true),
        ("[^a-c]x", "bx", false),
        ("[]a]", "a", false),
        ("a\\*", "a*", true),
        ("[a-]", "a", false),
        ("[\\]]", "]", true),
        ("*[", "x[", false),
        ("é?", "éè", true),
    ];
    for (p, n, want) in cases {
        assert_eq!(matches(p, n), want, "{p} {n}");
    }
}

/// Go's `TestHashChangesOnlyWithAnInput`, and the hash Go computes for the same files (so a task Go recorded as done
/// is not run again after the switch).
#[test]
fn the_hash_changes_only_with_an_input_and_is_gos() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    write(dir, "migrations/001.sql", "create table a;");
    write(dir, "migrations/002.sql", "create table b;");
    write(dir, "README.md", "hello");
    let p = pats(&["migrations/*.sql"]);
    let (h0, n) = hash(dir, &p);
    assert_eq!(
        (h0.as_str(), n),
        (
            "1a7caaac7b714dab51c7d0df6dc5e86b0585b98768416257315e9cf338ea8bec",
            2
        )
    );
    write(dir, "README.md", "changed outside the inputs");
    assert_eq!(hash(dir, &p).0, h0);
    write(dir, "migrations/002.sql", "create table b2;");
    let (h1, _) = hash(dir, &p);
    assert_ne!(h1, h0);
    write(dir, "migrations/003.sql", "create table c;");
    let (h2, n) = hash(dir, &p);
    assert!(h2 != h1 && n == 3);
    std::fs::remove_file(dir.join("migrations/001.sql")).unwrap();
    let (h3, n) = hash(dir, &p);
    assert!(h3 != h2 && n == 2);
}

/// Go's `TestNothingMatchedIsAStableHash`: the empty sha256.
#[test]
fn nothing_matched_is_a_stable_hash() {
    let t = tempfile::tempdir().unwrap();
    let p = pats(&["nope/**/*.x"]);
    let (a, n) = hash(t.path(), &p);
    assert_eq!(
        (a.as_str(), n),
        (
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            0
        )
    );
}

/// Go's `TestHeavyFoldersAreSkippedUnlessNamed`; a pattern naming one file, and `./`.
#[test]
fn heavy_folders_are_skipped_unless_named() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path();
    write(dir, "src/a.js", "1");
    write(dir, "node_modules/pkg/index.js", "2");
    write(dir, ".git/x.js", "3");
    assert_eq!(files(dir, &pats(&["**/*.js"])), ["src/a.js"]);
    assert_eq!(
        files(dir, &pats(&["node_modules/**/*.js"])),
        ["node_modules/pkg/index.js"]
    );
    assert_eq!(files(dir, &pats(&["./src/a.js"])), ["src/a.js"]);
}
