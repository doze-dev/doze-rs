//! What the help sheet says, apart from how it is drawn: each context's lines (generated from the action table, so it
//! lists exactly what is bound), folded where rows say the same thing, split into small groups where a context has
//! many, and what a filter finds across every context and the words.

use crate::actions::{self, Scope, Spec};

use super::groups;

/// The glossary's entries: the word, its tone, what it means.
pub(crate) type Word = (&'static str, &'static str, &'static str);

/// The words the sheet explains.
pub(crate) fn words() -> &'static [Word] {
    &super::super::glossary::WORDS
}

/// Runs of rows that fold into one line: the run's first label, how many rows, the line's label. Rows whose words
/// are just "move" fold on their own (↑ ↓ k j are one line).
const FOLDED: [(&str, usize, &str); 4] = [
    ("Tab 1", 9, "A stack's tab"),
    ("Tiles", 4, "Tiles · List · Map · Grid"),
    ("Debug and up", 4, "Level: debug to error"),
    ("Card above", 4, "The nearest card"),
];

/// One line of the sheet: its keys as chips write them, the keys it stands for, the rows it stands for, its words.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub(crate) keys: Vec<String>,
    pub(crate) raw: Vec<&'static str>,
    pub(crate) rows: Vec<&'static str>,
    pub(crate) label: String,
    pub(crate) what: &'static str,
}

/// What the sheet shows: one context's keys, or the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Keys(Scope),
    Words,
}

/// The rail, in its order: everywhere, then the pages as a person meets them, then the words.
pub(crate) const RAIL: [Section; 12] = [
    Section::Keys(Scope::Global),
    Section::Keys(Scope::Library),
    Section::Keys(Scope::Overview),
    Section::Keys(Scope::Instance),
    Section::Keys(Scope::Logs),
    Section::Keys(Scope::Map),
    Section::Keys(Scope::Grid),
    Section::Keys(Scope::Console),
    Section::Keys(Scope::Settings),
    Section::Keys(Scope::Palette),
    Section::Keys(Scope::Bell),
    Section::Words,
];

impl Section {
    /// Its name in the rail.
    pub(crate) fn short(self) -> &'static str {
        match self {
            Section::Words => "Words",
            Section::Keys(sc) => match sc {
                Scope::Global => "Everywhere",
                Scope::Library => "Home",
                Scope::Overview => "A stack's page",
                Scope::Instance => "A service's page",
                Scope::Logs => "Logs",
                Scope::Map => "Map",
                Scope::Grid => "Grid",
                Scope::Console => "A module's console",
                Scope::Settings => "Settings",
                Scope::Palette => "⌘K",
                Scope::Bell => "Needs you",
                _ => sc.title(),
            },
        }
    }

    /// Its heading over its keys.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Section::Words => "The words",
            Section::Keys(sc) => sc.title(),
        }
    }

    /// The section `by` steps along the rail, kept at its ends.
    pub(crate) fn step(self, by: isize) -> Section {
        let at = RAIL.iter().position(|s| *s == self).unwrap_or(0);
        let to = at.saturating_add_signed(by).min(RAIL.len() - 1);
        RAIL.get(to).copied().unwrap_or(self)
    }
}

/// The section the sheet opens on for the page's context: its keys, or Everywhere's for a context the sheet has no
/// keys for (a folder with no doze.hcl).
pub(crate) fn opening(here: Scope) -> Section {
    let s = Section::Keys(here);
    if RAIL.contains(&s) {
        s
    } else {
        Section::Keys(Scope::Global)
    }
}

/// `⌘1 ⌘2 … ⌘9` as `⌘1–9`; `1 2 3 4` as `1–4`; pairs that move one way and back as one chip (↑↓, ←→, j k, hjkl).
fn span_keys(keys: Vec<String>) -> Vec<String> {
    let digit = |k: &str| k.chars().last().is_some_and(|c| c.is_ascii_digit());
    if keys.len() > 3 && keys.iter().all(|k| digit(k)) {
        let first = keys.first().cloned().unwrap_or_default();
        let end: String = keys
            .last()
            .and_then(|l| l.chars().last())
            .map(String::from)
            .unwrap_or_default();
        return vec![format!("{first}–{end}")];
    }
    let mut out: Vec<String> = Vec::new();
    let has = |k: &str| keys.iter().any(|x| x == k);
    match (has("↑") && has("↓"), has("←") && has("→")) {
        (true, true) => out.push("↑↓←→".to_owned()),
        (true, false) => out.push("↑↓".to_owned()),
        (false, true) => out.push("←→".to_owned()),
        _ => {}
    }
    match (has("k") && has("j"), has("h") && has("l")) {
        (true, true) => out.push("hjkl".to_owned()),
        (true, false) => out.push("j k".to_owned()),
        _ => {}
    }
    let any = !out.is_empty();
    let paired = |k: &String| any && ["↑", "↓", "←", "→", "k", "j", "h", "l"].contains(&k.as_str());
    out.extend(keys.iter().filter(|k| !paired(k)).cloned());
    out
}

/// A context's rows, runs that say the same thing folded into one line.
pub(crate) fn lines(rows: &[&'static Spec]) -> Vec<Line> {
    // Each line, how many rows its run may still take, and whether it is a run of moves.
    let mut out: Vec<(Line, usize, bool)> = Vec::new();
    for s in rows {
        let keys: Vec<String> = s.keys.iter().map(|k| actions::key_words(k)).collect();
        let mover = s.what == "move";
        if let Some((last, left, moves)) = out.last_mut()
            && (*left > 0 || last.rows.last() == Some(&s.label) || (mover && *moves))
        {
            *left = left.saturating_sub(1);
            last.raw.extend(s.keys.iter().copied());
            last.rows.push(s.label);
            for k in keys {
                if !last.keys.contains(&k) {
                    last.keys.push(k);
                }
            }
            continue;
        }
        let fold = FOLDED.iter().find(|(f, _, _)| *f == s.label);
        let (label, what) = match fold {
            Some((_, _, l)) => ((*l).to_owned(), ""),
            None if mover => ("Move".to_owned(), ""),
            None => (s.label.to_owned(), s.what),
        };
        out.push((
            Line {
                keys,
                raw: s.keys.to_vec(),
                rows: vec![s.label],
                label,
                what,
            },
            fold.map_or(0, |(_, n, _)| n - 1),
            mover,
        ));
    }
    out.into_iter()
        .map(|(mut l, _, _)| {
            l.keys = span_keys(std::mem::take(&mut l.keys));
            l
        })
        .collect()
}

/// A context's lines, by scope.
pub(crate) fn scope_lines(sc: Scope) -> Vec<Line> {
    actions::help()
        .into_iter()
        .find(|(s, _)| *s == sc)
        .map(|(_, rows)| lines(&rows))
        .unwrap_or_default()
}

/// A run of lines under a small heading (none when the context is short enough to read as one list).
#[derive(Clone, Debug)]
pub(crate) struct Group {
    pub(crate) title: Option<&'static str>,
    pub(crate) lines: Vec<Line>,
}

/// Past this many lines a context is split into groups.
pub(crate) const GROUP_PAST: usize = 8;

/// A context's lines, in its groups when it has more than [`GROUP_PAST`]. A line no group names (a row added to the
/// table and not yet to [`groups::GROUPS`]) still shows, in a last group of its own, so no key can drop off the sheet.
pub(crate) fn grouped(sc: Scope) -> Vec<Group> {
    let all = scope_lines(sc);
    let named = groups::of(sc);
    if all.len() <= GROUP_PAST || named.is_empty() {
        return vec![Group {
            title: None,
            lines: all,
        }];
    }
    let mut left = all;
    let mut out: Vec<Group> = named
        .iter()
        .map(|(title, labels)| {
            let mut lines = Vec::new();
            for l in *labels {
                if let Some(i) = left.iter().position(|x| x.label == *l) {
                    lines.push(left.remove(i));
                }
            }
            Group {
                title: Some(*title),
                lines,
            }
        })
        .filter(|g| !g.lines.is_empty())
        .collect();
    if !left.is_empty() {
        out.push(Group {
            title: Some("More"),
            lines: left,
        });
    }
    out
}

/// Whether `text` holds `q` (already lowercased), and where, as a byte range of `text`.
pub(crate) fn hit(text: &str, q: &str) -> Option<std::ops::Range<usize>> {
    if q.is_empty() {
        return None;
    }
    let low = text.to_lowercase();
    let at = low.find(q)?;
    // Lowercasing kept every byte where it was (true of the sheet's words), so the range is the text's own.
    (low.len() == text.len()).then_some(at..at + q.len())
}

/// Whether a line answers `q` (lowercased, trimmed): by its words, its rows' labels, a key as its chip writes it or
/// as typed (`cmd-k`, `space`).
pub(crate) fn line_hits(l: &Line, q: &str) -> bool {
    hit(&l.label, q).is_some()
        || hit(l.what, q).is_some()
        || l.rows.iter().any(|r| hit(r, q).is_some())
        || l.keys.iter().any(|k| k.to_lowercase() == q)
        || l.raw
            .iter()
            .any(|k| *k == q || actions::key_words(k).to_lowercase() == q)
}

/// What a filter found: each context's matching lines (contexts in the rail's order), and the matching words.
#[derive(Debug, Default)]
pub(crate) struct Found {
    pub(crate) keys: Vec<(Scope, Vec<Line>)>,
    pub(crate) words: Vec<Word>,
}

impl Found {
    /// How many matches `s` has.
    pub(crate) fn count(&self, s: Section) -> usize {
        match s {
            Section::Words => self.words.len(),
            Section::Keys(sc) => self
                .keys
                .iter()
                .find(|(x, _)| *x == sc)
                .map_or(0, |(_, l)| l.len()),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.words.is_empty()
    }
}

/// Every line and word in any context that answers `query`.
pub(crate) fn find(query: &str) -> Found {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Found::default();
    }
    let keys = RAIL
        .iter()
        .filter_map(|s| match s {
            Section::Keys(sc) => Some(*sc),
            Section::Words => None,
        })
        .map(|sc| {
            let ls: Vec<Line> = scope_lines(sc)
                .into_iter()
                .filter(|l| line_hits(l, &q))
                .collect();
            (sc, ls)
        })
        .filter(|(_, ls)| !ls.is_empty())
        .collect();
    let words = words()
        .iter()
        .filter(|(w, _, m)| hit(w, &q).is_some() || hit(m, &q).is_some())
        .copied()
        .collect();
    Found { keys, words }
}
