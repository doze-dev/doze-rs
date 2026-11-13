//! What the logs' filter box says, read once: plain words, matched ignoring case as the Go page matched them, or a
//! pattern between slashes (`/timeout|refused/`), matched ignoring case unless it has a capital (smart case, as
//! ripgrep does). A pattern that does not compile says why and filters nothing, so a half-typed `/(` never empties
//! the page. Both are compiled to one automaton, so a line is matched, and its matches found for highlighting, the
//! same way.

use regex::{Regex, RegexBuilder};
use std::ops::Range;

/// The longest filter read; past it the rest is ignored.
const MAX: usize = 512;
/// How big a compiled pattern may grow: typed patterns are small, and a pathological one is refused, not built.
const SIZE: usize = 1 << 20;

/// The filter in force.
#[derive(Debug, Clone, Default)]
pub enum Filter {
    /// Nothing typed: every line.
    #[default]
    None,
    /// Words or a pattern.
    Match {
        re: Regex,
        /// Typed between slashes.
        pattern: bool,
    },
    /// A pattern that does not compile, and why.
    Bad(String),
}

impl PartialEq for Filter {
    fn eq(&self, other: &Filter) -> bool {
        match (self, other) {
            (Filter::None, Filter::None) => true,
            (Filter::Match { re: a, pattern: p }, Filter::Match { re: b, pattern: q }) => {
                p == q && a.as_str() == b.as_str()
            }
            (Filter::Bad(a), Filter::Bad(b)) => a == b,
            _ => false,
        }
    }
}

// fuzz:parser fuzz_filter
/// Reads what was typed.
pub fn parse(text: &str) -> Filter {
    let cut = text
        .char_indices()
        .nth(MAX)
        .map_or(text, |(i, _)| text.get(..i).unwrap_or(text));
    if cut.is_empty() {
        return Filter::None;
    }
    let (source, pattern) = match cut
        .strip_prefix('/')
        .and_then(|rest| rest.strip_suffix('/'))
    {
        Some(p) if !p.is_empty() => (p.to_owned(), true),
        _ => (regex::escape(cut), false),
    };
    let insensitive = !pattern || !source.chars().any(char::is_uppercase);
    match RegexBuilder::new(&source)
        .case_insensitive(insensitive)
        .size_limit(SIZE)
        .dfa_size_limit(SIZE)
        .build()
    {
        Ok(re) => Filter::Match { re, pattern },
        Err(e) => Filter::Bad(short(&e.to_string())),
    }
}

/// A compile error's last line, which says what is wrong (`error: unclosed group`).
fn short(e: &str) -> String {
    let last = e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(e);
    last.trim().trim_start_matches("error: ").to_owned()
}

impl Filter {
    /// Whether a line is kept.
    pub fn keeps(&self, line: &str) -> bool {
        match self {
            Filter::Match { re, .. } => re.is_match(line),
            Filter::None | Filter::Bad(_) => true,
        }
    }

    /// Whether anything is matched at all (a filter box with words or a pattern that compiled).
    pub fn active(&self) -> bool {
        matches!(self, Filter::Match { .. })
    }

    /// Where it matches in `line`, for highlighting: byte ranges, in order, none empty.
    pub fn spans(&self, line: &str) -> Vec<Range<usize>> {
        match self {
            Filter::Match { re, .. } => re
                .find_iter(line)
                .map(|m| m.range())
                .filter(|r| !r.is_empty())
                .take(64)
                .collect(),
            Filter::None | Filter::Bad(_) => Vec::new(),
        }
    }

    /// Why the pattern is refused, if it is.
    pub fn error(&self) -> Option<&str> {
        match self {
            Filter::Bad(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, parse};

    #[test]
    fn words_ignore_case_and_mean_what_they_say() {
        let f = parse("GET /cart");
        assert!(f.keeps("get /cart 200 44ms"));
        assert!(!f.keeps("GET /products"));
        // Slashes inside words are words: "a.b" is not a pattern.
        assert!(parse("a.b").keeps("x a.b y") && !parse("a.b").keeps("axb"));
        assert_eq!(parse(""), Filter::None);
        assert!(parse("").keeps("anything"));
    }

    #[test]
    fn a_pattern_between_slashes_is_smart_about_case() {
        let f = parse("/time(out|d)/");
        assert!(f.keeps("request TIMEOUT after 5s"));
        let strict = parse("/Timeout/");
        assert!(strict.keeps("Timeout") && !strict.keeps("timeout"));
        assert_eq!(parse("/a|b/").spans("xaxbx"), vec![1..2, 3..4]);
    }

    #[test]
    fn a_broken_pattern_says_why_and_keeps_everything() {
        let f = parse("/(unclosed/");
        assert!(f.error().is_some_and(|e| e.contains("unclosed")), "{f:?}");
        assert!(f.keeps("anything") && !f.active());
        // A lone slash, or two, is a word.
        assert!(parse("/").keeps("a/b") && parse("//").keeps("http://x"));
    }

    #[test]
    fn empty_matches_are_not_highlighted() {
        assert!(parse("/x*/").spans("abc").is_empty());
        let long = "y".repeat(10_000);
        assert!(parse(&long).active());
    }
}
