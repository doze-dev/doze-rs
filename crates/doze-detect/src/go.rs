//! Go: go.mod's requirements, the module's main packages (the root and `cmd/*`), and in each main package the imports,
//! the environment variables it reads and a port written in it.

use crate::files::{read_dir_sorted, slug};
use crate::{App, Scan};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// `os.Getenv("X")`, `os.LookupEnv("X")`, `getenv("X")`, `env("X")` (Go's `reGetenv`, used for JavaScript too).
pub(crate) static GETENV: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#"(?:os\.Getenv|os\.LookupEnv|getenv|env)\(\s*"([A-Z][A-Z0-9_]+)""#));
static IMPORT: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#""([a-zA-Z0-9.\-]+\.[a-z]+/[^"\s]+)""#));
static PORT_LITERAL: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r#"(?:ListenAndServe\(\s*"[^"]*:|"PORT",\s*")(\d{2,5})""#));
static PACKAGE_MAIN: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"(?m)^package\s+main\b"));

/// The modules go.mod requires, in the file's order: the lines of a `require ( … )` block, and (unlike Go's
/// detector, which missed them: proposal F6) one-line `require path version` statements.
// fuzz:parser fuzz_project_files
pub fn requires(gomod: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in gomod.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f.as_slice() {
            ["require", path, _, ..] if *path != "(" => out.push((*path).to_owned()),
            [first, _, ..]
                if !first.starts_with("//")
                    && !matches!(*first, "module" | "go" | "require")
                    && first.contains('.') =>
            {
                out.push((*first).to_owned());
            }
            _ => {}
        }
    }
    out
}

/// Every distinct capture of `re`'s first group in `text`, sorted.
pub(crate) fn captures(re: &LazyLock<Option<Regex>>, text: &str) -> Vec<String> {
    let mut out: Vec<String> = re
        .as_ref()
        .map(|r| {
            r.captures_iter(text)
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

pub(crate) fn scan(s: &mut Scan<'_>, dir: &Path, abs: &Path, apps: &mut Vec<App>) {
    let Ok(gomod) = std::fs::read_to_string(dir.join("go.mod")) else {
        return;
    };
    s.note_go_packages(&requires(&gomod), "go.mod requires");
    for (name, rel) in mains(dir, abs) {
        let (src, imports) = sources(&dir.join(&rel));
        s.note_go_packages(&imports, "the code imports");
        let envs = captures(&GETENV, &src);
        let port = PORT_LITERAL
            .as_ref()
            .and_then(|r| r.captures(&src))
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        apps.push(App {
            name,
            run: if rel == "." {
                "go run .".into()
            } else {
                format!("go run ./{rel}")
            },
            port,
            reads_port: envs.iter().any(|e| e == "PORT"),
            why: format!(
                "a Go main package at {}",
                if rel == "." { "the project root" } else { &rel }
            ),
            uses: s.uses(&envs),
        });
    }
}

/// The main packages: the root (named after the folder), then each `cmd/<name>`.
fn mains(dir: &Path, abs: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if has_main(dir) {
        let base = abs
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        out.push((slug(&base), ".".to_owned()));
    }
    for (name, path) in read_dir_sorted(&dir.join("cmd")) {
        if path.is_dir() && has_main(&path) {
            out.push((slug(&name), format!("cmd/{name}")));
        }
    }
    out
}

fn go_files(dir: &Path) -> impl Iterator<Item = std::path::PathBuf> {
    read_dir_sorted(dir)
        .into_iter()
        .filter(|(n, p)| n.ends_with(".go") && !n.ends_with("_test.go") && p.is_file())
        .map(|(_, p)| p)
}

fn has_main(dir: &Path) -> bool {
    go_files(dir).any(|p| {
        std::fs::read_to_string(p)
            .is_ok_and(|src| PACKAGE_MAIN.as_ref().is_some_and(|r| r.is_match(&src)))
    })
}

/// A package's source (not its tests), and the import paths its import sections name.
fn sources(dir: &Path) -> (String, Vec<String>) {
    let mut all = String::new();
    let mut imports = Vec::new();
    for p in go_files(dir) {
        let Ok(src) = std::fs::read_to_string(p) else {
            continue;
        };
        all.push_str(&src);
        all.push('\n');
        if let Some(r) = IMPORT.as_ref() {
            imports.extend(
                r.captures_iter(import_block(&src))
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned())),
            );
        }
    }
    (all, imports)
}

/// A file's import section, so string literals elsewhere are not taken for import paths.
fn import_block(src: &str) -> &str {
    let Some(rest) = src.find("import").and_then(|i| src.get(i..)) else {
        return "";
    };
    match rest.find("\nfunc ") {
        Some(j) => rest.get(..j).unwrap_or(rest),
        None => rest,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn requirements_in_a_block_and_on_one_line() {
        let m = "module x\n\ngo 1.22\n\nrequire github.com/redis/go-redis/v9 v9.5.1\n\nrequire (\n\tgithub.com/jackc/pgx/v5 v5.5.0\n\t// a comment\n\tgolang.org/x/sync v0.6.0 // indirect\n)\n";
        assert_eq!(
            requires(m),
            [
                "github.com/redis/go-redis/v9",
                "github.com/jackc/pgx/v5",
                "golang.org/x/sync"
            ]
        );
    }

    #[test]
    fn imports_come_from_the_import_section_only() {
        let src = "package main\n\nimport (\n\t\"fmt\"\n\t\"github.com/lib/pq\"\n)\n\nfunc main() { fmt.Println(\"github.com/not/this\") }\n";
        let got: Vec<_> = IMPORT
            .as_ref()
            .unwrap()
            .captures_iter(import_block(src))
            .map(|c| c.get(1).unwrap().as_str().to_owned())
            .collect();
        assert_eq!(got, ["github.com/lib/pq"]);
    }
}
