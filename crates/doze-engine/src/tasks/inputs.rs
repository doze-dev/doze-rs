//! The files a task lists as its `inputs`, and their hash (Go's `internal/inputs`, its hash kept byte for byte so a
//! task Go recorded as done is not run again after the switch). A task runs again when the hash changes; the watcher
//! uses the same matcher to decide which changes matter.
//!
//! A pattern is a path relative to the task's folder. `*` and `?` match within one path segment, `[…]` is a class (Go's
//! `path.Match`), and a segment of `**` matches any number of segments, none included.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Folders a pattern walks into only by naming them.
const SKIPPED: [&str; 3] = [".git", "node_modules", ".doze"];

// fuzz:parser fuzz_glob
/// Whether the slash-separated relative path `name` matches `pattern`.
pub fn matches(pattern: &str, name: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let name: Vec<&str> = name.split('/').collect();
    let memo = vec![None; (pat.len() + 1) * (name.len() + 1)];
    Segs {
        pat: &pat,
        name: &name,
        memo,
    }
    .at(0, 0)
}

/// Segment matching, each (pattern segment, name segment) pair decided once: a pattern of many `**` against a deep
/// path would otherwise try every split at every `**` (exponential; the fuzzer's timeout found it).
struct Segs<'a> {
    pat: &'a [&'a str],
    name: &'a [&'a str],
    memo: Vec<Option<bool>>,
}

impl Segs<'_> {
    fn at(&mut self, pi: usize, ni: usize) -> bool {
        let key = pi * (self.name.len() + 1) + ni;
        if let Some(Some(v)) = self.memo.get(key) {
            return *v;
        }
        let v = self.decide(pi, ni);
        if let Some(slot) = self.memo.get_mut(key) {
            *slot = Some(v);
        }
        v
    }

    fn decide(&mut self, pi: usize, ni: usize) -> bool {
        let Some(first) = self.pat.get(pi) else {
            return ni >= self.name.len();
        };
        if *first == "**" {
            // A run of `**` is one.
            let mut next = pi + 1;
            while self.pat.get(next) == Some(&"**") {
                next += 1;
            }
            if next >= self.pat.len() {
                return true;
            }
            for i in ni..=self.name.len() {
                if self.at(next, i) {
                    return true;
                }
            }
            return false;
        }
        let Some(seg) = self.name.get(ni) else {
            return false;
        };
        let p: Vec<char> = first.chars().collect();
        let s: Vec<char> = seg.chars().collect();
        segment(&p, &s) && self.at(pi + 1, ni + 1)
    }
}

/// Go's `path.Match` within one segment, by its chunk algorithm: each run of stars is followed by a chunk of literal
/// characters, classes and `?`, and the first place a chunk matches is taken, trying later places only while the rest
/// fails. That keeps it linear in the stars (a naive backtracker took seconds on `*a*a*a*a*a*a*a*a*b`, E2 audit 1, M2).
/// A malformed pattern matches nothing.
fn segment(p: &[char], s: &[char]) -> bool {
    let (mut p, mut s) = (p, s);
    'chunks: while !p.is_empty() {
        let (star, chunk, rest) = scan_chunk(p);
        p = rest;
        if star && chunk.is_empty() {
            return true; // a trailing star takes the rest of the segment
        }
        if let Some(t) = match_chunk(chunk, s)
            && (t.is_empty() || !p.is_empty())
        {
            s = t;
            continue;
        }
        if star {
            for i in 0..s.len() {
                if let Some(t) = match_chunk(chunk, s.get(i + 1..).unwrap_or_default()) {
                    if p.is_empty() && !t.is_empty() {
                        continue;
                    }
                    s = t;
                    continue 'chunks;
                }
            }
        }
        return false;
    }
    s.is_empty()
}

/// Splits off the stars at the front, then the chunk up to the next star outside a class.
fn scan_chunk(p: &[char]) -> (bool, &[char], &[char]) {
    let start = p.iter().take_while(|c| **c == '*').count();
    let p = p.get(start..).unwrap_or_default();
    let (mut i, mut in_class) = (0, false);
    while let Some(c) = p.get(i) {
        match c {
            '\\' => i += 1,
            '[' => in_class = true,
            ']' => in_class = false,
            '*' if !in_class => break,
            _ => {}
        }
        i += 1;
    }
    let i = i.min(p.len());
    (
        start > 0,
        p.get(..i).unwrap_or_default(),
        p.get(i..).unwrap_or_default(),
    )
}

/// Matches `chunk` against the front of `s`: what is left of `s`, or `None`.
fn match_chunk<'s>(mut chunk: &[char], mut s: &'s [char]) -> Option<&'s [char]> {
    while let Some((pc, rest)) = chunk.split_first() {
        let (c, s_rest) = s.split_first()?;
        chunk = match pc {
            '[' => {
                let (set, after) = class(rest)?;
                if !set.has(*c) {
                    return None;
                }
                after
            }
            '?' => rest,
            '\\' => {
                let (e, after) = rest.split_first()?;
                if e != c {
                    return None;
                }
                after
            }
            pc => {
                if pc != c {
                    return None;
                }
                rest
            }
        };
        s = s_rest;
    }
    Some(s)
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
    found(dir, patterns)
        .into_keys()
        .map(|rel| String::from_utf8_lossy(&rel).into_owned())
        .collect()
}

/// What [`files`] finds, keyed by the relative path's bytes (a name that is not UTF-8 is kept as Go keeps it, E2 audit
/// 1, m5; it is matched as text with its invalid bytes replaced), with where each file is.
fn found(dir: &Path, patterns: &[String]) -> BTreeMap<Vec<u8>, PathBuf> {
    let mut found = BTreeMap::new();
    for pat in patterns {
        // Go's `filepath.ToSlash`: only where a backslash is the separator; elsewhere it escapes the next character.
        let pat = if std::path::MAIN_SEPARATOR == '\\' {
            pat.replace('\\', "/")
        } else {
            pat.clone()
        };
        let pat = pat.strip_prefix("./").unwrap_or(&pat);
        let base = root(pat);
        // Go's `filepath.Join`: a pattern is always under the folder, a leading `/` included (E2 audit 4), and on
        // Windows a drive letter too (`C:/x` is a name under the folder, not another drive): appended, never joined,
        // since a joined absolute path would replace the folder.
        let start = under(dir, base.trim_start_matches('/'));
        let Ok(meta) = std::fs::metadata(&start) else {
            continue; // nothing there yet: nothing matches
        };
        // Go's `WalkDir` does not walk into a root that is a link: it is matched as itself (E2 audit 4).
        let link = std::fs::symlink_metadata(&start).is_ok_and(|m| m.file_type().is_symlink());
        if !meta.is_dir() || link {
            if matches(pat, &base) {
                found.insert(base.clone().into_bytes(), start);
            }
            continue;
        }
        walk(&start, base.as_bytes(), pat, &mut found);
    }
    found
}

/// `rel` (slash-separated) under `dir`, whatever it looks like.
fn under(dir: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        return dir.to_path_buf();
    }
    let mut s = dir.as_os_str().to_owned();
    s.push(std::path::MAIN_SEPARATOR_STR);
    s.push(rel);
    PathBuf::from(s)
}

fn walk(at: &Path, rel: &[u8], pat: &str, found: &mut BTreeMap<Vec<u8>, PathBuf>) {
    let Ok(entries) = std::fs::read_dir(at) else {
        return;
    };
    for e in entries.flatten() {
        let os = e.file_name();
        let name = os.to_string_lossy();
        let mut child = rel.to_vec();
        if !rel.is_empty() {
            child.push(b'/');
        }
        child.extend_from_slice(os.as_encoded_bytes());
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            if !(SKIPPED.contains(&name.as_ref()) && !pat.contains(name.as_ref())) {
                walk(&e.path(), &child, pat, found);
            }
        } else if matches(pat, &String::from_utf8_lossy(&child)) {
            found.insert(child, e.path());
        }
    }
}

/// A digest of the paths and contents of the files the patterns match, and how many there are; Go's: sha256 over each
/// file's path, a zero byte and its content's sha256, in path order. No match is a valid, stable hash.
pub fn hash(dir: &Path, patterns: &[String]) -> (String, usize) {
    hash_unless(dir, patterns, &std::sync::atomic::AtomicBool::new(false)).unwrap_or_default()
}

/// [`hash`], given up (`None`) as soon as `abandon` is set: a task stopped while it decides does not leave a thread
/// reading a large input to the end, holding one of the daemon's few blocking threads and its shutdown.
pub fn hash_unless(
    dir: &Path,
    patterns: &[String],
    abandon: &std::sync::atomic::AtomicBool,
) -> Option<(String, usize)> {
    let files = found(dir, patterns);
    let mut h = Sha256::new();
    for (rel, path) in &files {
        // Streamed, as Go's `io.Copy`: a large input costs a buffer, not its size (E2 audit 1, M4).
        let digest = match file_digest(path, abandon) {
            Part::Abandoned => return None,
            Part::Gone => continue, // gone between the walk and the read, or not a regular file
            Part::Of(d) => d,
        };
        h.update(rel);
        h.update([0]);
        h.update(digest);
    }
    Some((hex::encode(h.finalize()), files.len()))
}

/// One input's part of the hash.
enum Part {
    Of(Vec<u8>),
    Gone,
    Abandoned,
}

fn file_digest(path: &Path, abandon: &std::sync::atomic::AtomicBool) -> Part {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    // Only a regular file is read, checked before the open: opening a FIFO for reading is itself what waits, for a
    // writer that may never come (Go's hash hung there, and with it the task's stop and the daemon's exit), and a
    // device may never end. Anything else is skipped as an unreadable file is (a folder always was: its read fails).
    match std::fs::metadata(path) {
        Err(_) => return Part::Gone,
        Ok(m) if !m.is_file() => return Part::Gone,
        Ok(_) => {}
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return Part::Gone;
    };
    let mut fh = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if abandon.load(Ordering::Relaxed) {
            return Part::Abandoned;
        }
        match f.read(&mut buf) {
            Ok(0) => return Part::Of(fh.finalize().to_vec()),
            Ok(n) => fh.update(buf.get(..n).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Part::Gone,
        }
    }
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
