//! A log line's severity, read from what the program wrote. Programs print what they like, so this is a reading of
//! the common shapes and not a promise: a level word near the front (`[warn]`, `ERROR:`, `| info |`, the `LOG:`
//! postgres puts after its prefix), logfmt (`level=warn`, `lvl=`, `severity=`), and what JSON loggers write
//! (`{"level":"error"}`, `"severity":"WARNING"`, and pino's numbers, 30 for info).

use crate::gonum::lower;

/// A log line's level, least severe first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    /// Debug and trace.
    Debug,
    /// Info, notice, and a line that says nothing.
    Info,
    /// Warnings.
    Warn,
    /// Errors, panics and fatals.
    Error,
}

impl LogLevel {
    /// Every level, least severe first.
    pub const ALL: [Self; 4] = [Self::Debug, Self::Info, Self::Warn, Self::Error];

    /// The level's word: `debug`, `info`, `warn`, `error`.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// Its three-letter tag: `DBG`, `INF`, `WRN`, `ERR`.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Debug => "DBG",
            Self::Info => "INF",
            Self::Warn => "WRN",
            Self::Error => "ERR",
        }
    }

    /// A logs filter that shows this level and up, as the filter reads it. (Go: `LevelNames`.)
    #[must_use]
    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Debug => "debug and up",
            Self::Info => "info and up",
            Self::Warn => "warnings and up",
            Self::Error => "errors only",
        }
    }
}

/// The words a level is written as, in the order they are looked for.
const WORDS: [(&str, LogLevel); 16] = [
    ("fatal", LogLevel::Error),
    ("panic", LogLevel::Error),
    ("critical", LogLevel::Error),
    ("error", LogLevel::Error),
    ("err", LogLevel::Error),
    ("warning", LogLevel::Warn),
    ("warn", LogLevel::Warn),
    ("wrn", LogLevel::Warn),
    ("info", LogLevel::Info),
    ("inf", LogLevel::Info),
    ("notice", LogLevel::Info),
    ("log", LogLevel::Info),
    ("debug", LogLevel::Debug),
    ("dbg", LogLevel::Debug),
    ("trace", LogLevel::Debug),
    ("trc", LogLevel::Debug),
];

// fuzz:parser fuzz_level
/// The level a line says, or `None` for a line that says nothing. (Go: `Level`.)
#[must_use]
pub fn level(line: &str) -> Option<LogLevel> {
    // The first 120 bytes of the line lowered, as Go lowers the whole line and then cuts it: on the stack, since
    // this runs for every line a logs view draws.
    let mut buf = [0_u8; HEAD + 4];
    let mut n = 0;
    for c in line.chars() {
        if n >= HEAD {
            break;
        }
        let l = lower(c);
        if let Some(slot) = buf.get_mut(n..n + l.len_utf8()) {
            l.encode_utf8(slot);
        }
        n += l.len_utf8();
    }
    let head = buf.get(..n.min(HEAD)).unwrap_or_default();
    keyed(head).or_else(|| near_front(head.get(..60).unwrap_or(head)))
}

const HEAD: usize = 120;

/// A level under a key, wherever it falls: JSON (`"level":`) or logfmt (`level=`).
fn keyed(head: &[u8]) -> Option<LogLevel> {
    for key in ["level", "lvl", "severity", "loglevel"] {
        let key = key.as_bytes();
        for pre in [
            [b"\"", key, b"\":"].as_slice(),
            &[b"\"", key, b"\": "],
            &[key, b"="],
        ] {
            let Some(at) = find(head, pre) else {
                continue;
            };
            let rest = head
                .get(at + pre.iter().map(|p| p.len()).sum::<usize>()..)
                .unwrap_or_default();
            let start = rest
                .iter()
                .position(|&b| b != b'"' && b != b' ')
                .unwrap_or(rest.len());
            let val = rest.get(start..).unwrap_or_default();
            let end = val
                .iter()
                .position(|b| b"\", }".contains(b))
                .unwrap_or(val.len());
            if let Some(l) = value_level(val.get(..end).unwrap_or_default()) {
                return Some(l);
            }
        }
    }
    None
}

/// A level word near the front: `[warn]`, ` warn `, `|warn|`, ` warn:`, or leading the line.
fn near_front(head: &[u8]) -> Option<LogLevel> {
    WORDS.iter().find_map(|&(word, level)| {
        let w = word.as_bytes();
        let found = [(b"[", b"]"), (b" ", b" "), (b"|", b"|"), (b" ", b":")]
            .iter()
            .any(|(open, close)| find(head, &[*open, w, *close]).is_some())
            || [b" ", b":"].iter().any(|end| starts_with(head, &[w, *end]));
        found.then_some(level)
    })
}

/// A value found under a level key: a word, or pino's numbers.
fn value_level(v: &[u8]) -> Option<LogLevel> {
    match v {
        b"10" | b"20" => Some(LogLevel::Debug),
        b"30" => Some(LogLevel::Info),
        b"40" => Some(LogLevel::Warn),
        b"50" | b"60" => Some(LogLevel::Error),
        _ => WORDS
            .iter()
            .find(|(w, _)| w.as_bytes() == v)
            .map(|&(_, l)| l),
    }
}

/// Where `parts`, one after another, first occur in `hay`.
fn find(hay: &[u8], parts: &[&[u8]]) -> Option<usize> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    (0..=hay.len().checked_sub(len)?)
        .find(|&i| starts_with(hay.get(i..).unwrap_or_default(), parts))
}

fn starts_with(mut s: &[u8], parts: &[&[u8]]) -> bool {
    for part in parts {
        match s.strip_prefix(*part) {
            Some(rest) => s = rest,
            None => return false,
        }
    }
    true
}

/// A line's level for a logs filter, and its tag: a line that says nothing counts as info, with no tag, so it is
/// shown by default and hidden by "warnings and up". (Go: `Rank`.)
#[must_use]
pub fn rank(line: &str) -> (LogLevel, Option<&'static str>) {
    match level(line) {
        Some(l) => (l, Some(l.tag())),
        None => (LogLevel::Info, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go: TestLevel. Level reads the shapes programs commonly log in, and says nothing about a line that does not
    // carry one, so a logs view can tell "info" from "unknown".
    #[test]
    fn level_reads_common_shapes() {
        use LogLevel::{Debug, Error, Info, Warn};
        for (line, want) in [
            ("INF listening addr=:8080", Some(Info)),
            ("[error] upstream timed out", Some(Error)),
            ("ERROR: relation \"x\" does not exist", Some(Error)),
            (
                "2026-10-11 14:04:00.123 UTC [612] LOG:  checkpoint complete",
                Some(Info),
            ),
            (
                "2026-10-11 14:04:00.123 UTC [612] WARNING:  too many",
                Some(Warn),
            ),
            ("level=warn msg=\"slow\" ms=212", Some(Warn)),
            ("ts=1 lvl=debug msg=x", Some(Debug)),
            (r#"{"level":"error","msg":"boom"}"#, Some(Error)),
            (r#"{"time":1,"msg":"x","level": "INFO"}"#, Some(Info)),
            (r#"{"severity":"WARNING","message":"x"}"#, Some(Warn)),
            (r#"{"level":50,"msg":"pino error"}"#, Some(Error)),
            (r#"{"level":30,"msg":"pino info"}"#, Some(Info)),
            ("GET /products 200 12ms", None),
            ("listening on :3000", None),
            ("", None),
        ] {
            assert_eq!(level(line), want, "level({line:?})");
        }
    }
}
