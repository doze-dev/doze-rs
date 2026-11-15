//! Log lines as the logs pages draw them (Go `view.LogsOf`): the time two ways, the instance on the merged page,
//! the level as a tag with the word it came from dropped from the text, doze's own events as `zzz` lines. Which
//! lines are shown, kept up as lines arrive, is [`super::logindex::LogIndex`].

use super::Ink;
use super::time::{self, Zone};
use crate::ring::{LineRef, LogRing};
use doze_display::{self as words, LogLevel};
use std::collections::BTreeSet;
use std::time::SystemTime;

/// The level filter's names, by [`LogLevel`] (Go `view.Levels`).
pub const LEVELS: [&str; 4] = [
    "debug and up",
    "info and up",
    "warnings and up",
    "errors only",
];

/// The level segment's short names.
pub const LEVEL_TABS: [&str; 4] = ["Debug", "Info+", "Warn+", "Errors"];

/// The instance doze's own events are shown as.
pub const DOZE: &str = "doze";

/// One line as the page draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// Whose line, on the merged page; empty on one instance's.
    pub instance: String,
    /// `19:20:40`, in the person's zone.
    pub time: String,
    /// `-3m 05s`.
    pub rel: String,
    /// What it said, without a leading level word its tag already says.
    pub text: String,
    /// `INF`, `WRN`, `ERR`, `DBG`; `zzz` for doze's own; empty for a line that says none.
    pub level: &'static str,
    pub rank: u8,
    pub run: u32,
    /// One of doze's own events.
    pub doze: bool,
}

impl Line {
    /// The ink of its text: errors red, warnings amber.
    pub fn ink(&self) -> Ink {
        match self.rank {
            3.. => Ink::Bad,
            2 => Ink::Warn,
            _ => Ink::Text,
        }
    }

    /// The ink of its level tag.
    pub fn level_ink(&self) -> Ink {
        if self.doze {
            return Ink::Accent;
        }
        match self.rank {
            3.. => Ink::Bad,
            2 => Ink::Warn,
            0 => Ink::Dim,
            _ => Ink::Cool,
        }
    }

    /// The line as copied: time, who, level, text, two spaces apart.
    pub fn copied(&self, clock: bool) -> String {
        let t = if clock { &self.time } else { &self.rel };
        [
            t.as_str(),
            self.instance.as_str(),
            self.level,
            self.text.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("  ")
    }
}

/// A level as the filters count it: 0 debug … 3 errors.
pub fn rank_of(l: LogLevel) -> u8 {
    match l {
        LogLevel::Debug => 0,
        LogLevel::Info => 1,
        LogLevel::Warn => 2,
        LogLevel::Error => 3,
    }
}

/// A ring line as drawn. `merged` names its instance.
pub fn line_of(l: &LineRef<'_>, merged: bool, now: SystemTime, zone: Zone) -> Line {
    let (level, tag) = words::rank(l.line);
    let tag = tag.unwrap_or("");
    Line {
        instance: if merged {
            l.instance.to_owned()
        } else {
            String::new()
        },
        time: l.time.map(|t| zone.clock(t)).unwrap_or_default(),
        rel: time::rel(l.time, now),
        text: without_level_word(l.line, tag).to_owned(),
        level: tag,
        rank: rank_of(level),
        run: l.run,
        doze: false,
    }
}

/// One of doze's own events as a line of the merged page.
pub fn event_line(e: &DozeLine, now: SystemTime, zone: Zone) -> Line {
    Line {
        instance: DOZE.to_owned(),
        time: e.time.map(|t| zone.clock(t)).unwrap_or_default(),
        rel: time::rel(e.time, now),
        text: e.text.clone(),
        level: "zzz",
        rank: 1,
        run: 0,
        doze: true,
    }
}

/// One of doze's own events, as the merged page holds it: `api woke (a connection)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DozeLine {
    pub time: Option<SystemTime>,
    pub text: String,
}

impl DozeLine {
    /// An event of the journal.
    pub fn of(e: &doze_control::Event) -> DozeLine {
        let w = crate::convert::event(e);
        DozeLine {
            time: crate::convert::time(&e.time),
            text: format!("{} {}", e.instance, words::sentence(&w)),
        }
    }
}

/// The run numbers an instance's lines carry, oldest first.
pub fn runs_of(ring: &LogRing, instance: &str) -> Vec<u32> {
    let runs: BTreeSet<u32> = ring
        .of(instance)
        .map(|l| l.run)
        .filter(|&r| r > 0)
        .collect();
    runs.into_iter().collect()
}

/// A leading level word the tag already says, dropped: `INFO GET /products` beside its `INF` tag reads
/// `GET /products`. Only a bare word at the very start is dropped. (Go `withoutLevelWord`.)
pub fn without_level_word<'a>(line: &'a str, tag: &str) -> &'a str {
    if tag.is_empty() || tag == "zzz" {
        return line;
    }
    let trimmed = line.trim_start_matches(' ');
    for w in [
        "INFO", "WARN", "WARNING", "ERROR", "DEBUG", "INF", "WRN", "ERR", "DBG",
    ] {
        let Some(head) = trimmed.get(..w.len()) else {
            continue;
        };
        let next = trimmed.as_bytes().get(w.len()).copied();
        if head.eq_ignore_ascii_case(w) && matches!(next, Some(b' ' | b'\t')) {
            return trimmed
                .get(w.len()..)
                .unwrap_or("")
                .trim_start_matches([' ', '\t']);
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::without_level_word;

    #[test]
    fn only_a_bare_leading_level_word_goes() {
        assert_eq!(
            without_level_word("INFO GET /products", "INF"),
            "GET /products"
        );
        assert_eq!(without_level_word("  warn\tslow", "WRN"), "slow");
        assert_eq!(without_level_word("INFORMATION x", "INF"), "INFORMATION x");
        assert_eq!(without_level_word("INFO", "INF"), "INFO");
        assert_eq!(without_level_word("INFO x", ""), "INFO x");
        assert_eq!(without_level_word("é INFO x", "INF"), "é INFO x");
    }
}
