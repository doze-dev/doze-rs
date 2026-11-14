//! The files a task lists as its `inputs`, and their hash (Go's `internal/inputs`, its hash kept byte for byte so a
//! task Go recorded as done is not run again after the switch). A task runs again when the hash changes; the watcher
//! uses the same matcher to decide which changes matter.
//!
//! A pattern is a path relative to the task's folder. `*` and `?` match within one path segment, `[…]` is a class (Go's
//! `path.Match`), and a segment of `**` matches any number of segments, none included.

use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// Folders a pattern walks into only by naming them.
const SKIPPED: [&str; 3] = [".git", "node_modules", ".doze"];

/// Whether the slash-separated relative path `name` matches `pattern`.
pub fn matches(pattern: &str, name: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let name: Vec<&str> = name.split('/').collect();
    match_segs(&pat, &name)
}

fn match_segs(pat: &[&str], name: &[&str]) -> bool {
    let Some((first, rest)) = pat.split_first() else {
        return name.is_empty();
    };
    if *first == "**" {
        // A run of `**` is one.
        let rest: Vec<&str> = rest.iter().copied().skip_while(|s| *s == "**").collect();
        if rest.is_empty() {
            return true;
        }
        return (0..=name.len()).any(|i| match_segs(&rest, name.get(i..).unwrap_or_default()));
    }
    let Some((seg, name_rest)) = name.split_first() else {
        return false;
    };
    let p: Vec<char> = first.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    segment(&p, &s) && match_segs(rest, name_rest)
}

/// Go's `path.Match` within one segment; a malformed pattern matches nothing.
fn segment(p: &[char], s: &[char]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some(('*', rest)) => (0..=s.len()).any(|i| segment(rest, s.get(i..).unwrap_or_default())),
        Some(('?', rest)) => s
            .split_first()
            .is_some_and(|(_, s_rest)| segment(rest, s_rest)),
        Some(('[', rest)) => match (class(rest), s.split_first()) {
            (Some((set, after)), Some((c, s_rest))) => set.has(*c) && segment(after, s_rest),
            _ => false,
        },
        Some(('\\', rest)) => match (rest.split_first(), s.split_first()) {
            (Some((pc, p_rest)), Some((c, s_rest))) => pc == c && segment(p_rest, s_rest),
            _ => false,
        },
        Some((pc, rest)) => s
            .split_first()
            .is_some_and(|(c, s_rest)| pc == c && segment(rest, s_rest)),
    }
}

/// A `[…]` class: its ranges and whether it is negated.
struct Class {
    negated: bool,
    ranges: Vec<(char, char)>,
}

impl Class {
    fn has(&self, c: char) -> bool {
        self.ranges.iter().any(|(lo, hi)| *lo <= c && c <= *hi) != self.negated
    }
}

/// Parses a class after its `[`: the class and what follows its `]`. `None` when malformed (Go's `ErrBadPattern`).
fn class(mut p: &[char]) -> Option<(Class, &[char])> {
    let mut cls = Class {
        negated: false,
        ranges: Vec::new(),
    };
    if let Some(('^', rest)) = p.split_first() {
        cls.negated = true;
        p = rest;
    }
    loop {
        if let Some((']', rest)) = p.split_first()
            && !cls.ranges.is_empty()
        {
            return Some((cls, rest));
        }
        let (lo, rest) = escaped(p)?;
        p = rest;
        let mut hi = lo;
        if let Some(('-', rest)) = p.split_first() {
            let (h, rest) = escaped(rest)?;
            hi = h;
            p = rest;
        }
        cls.ranges.push((lo, hi));
    }
}

/// One character of a class, `\` escaping the next; `-`, `]` or the end are malformed here.
fn escaped(p: &[char]) -> Option<(char, &[char])> {
    match p.split_first()? {
        ('-' | ']', _) => None,
        ('\\', rest) => rest.split_first().map(|(c, r)| (*c, r)),
        (c, rest) => Some((*c, rest)),
    }
}

/// The folder a pattern can match under: its segments before the first with a wildcard.
fn root(pattern: &str) -> String {
    let keep: Vec<&str> = pattern
        .split('/')
        .take_while(|seg| !seg.contains(['*', '?', '[']))
        .collect();
    keep.join("/")
}

/// The files under `dir` that any pattern matches, as sorted slash-separated paths relative to `dir`. Folders that
/// are not there yet match nothing; `.git`, `node_modules` and `.doze` are walked only by a pattern that names them;
/// links are not followed into.
pub fn files(dir: &Path, patterns: &[String]) -> Vec<String> {
    let mut found = BTreeSet::new();
    for pat in patterns {
        let pat = pat.replace('\\', "/");
        let pat = pat.strip_prefix("./").unwrap_or(&pat);
        let base = root(pat);
        let start = dir.join(&base);
        let Ok(meta) = std::fs::metadata(&start) else {
            continue; // nothing there yet: nothing matches
        };
        if !meta.is_dir() {
            if matches(pat, &base) {
                found.insert(base);
            }
            continue;
        }
        walk(&start, &base, pat, &mut found);
    }
    found.into_iter().collect()
}

fn walk(at: &Path, rel: &str, pat: &str, found: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(at) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            if !(SKIPPED.contains(&name.as_str()) && !pat.contains(&name)) {
                walk(&e.path(), &child, pat, found);
            }
        } else if matches(pat, &child) {
            found.insert(child);
        }
    }
}

/// A digest of the paths and contents of the files the patterns match, and how many there are; Go's: sha256 over each
/// file's path, a zero byte and its content's sha256, in path order. No match is a valid, stable hash.
pub fn hash(dir: &Path, patterns: &[String]) -> (String, usize) {
    let files = files(dir, patterns);
    let mut h = Sha256::new();
    for rel in &files {
        let Ok(body) = std::fs::read(dir.join(rel)) else {
            continue; // gone between the walk and the read
        };
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(Sha256::digest(&body));
    }
    (hex::encode(h.finalize()), files.len())
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
