//! Filter patterns: the language of `FilterLogEvents`, metric filters and subscription filters.
//!
//! Three forms, told apart by the first character: terms (`ERROR -debug ?WARN "disk full"`), a JSON expression
//! (`{ $.code = 507 && $.level = "error" }`) and a space-delimited expression (`[ip, user, ..., status = 4*, size]`).
//! The empty pattern matches every event. A pattern with a regular expression (`%...%`) is not built: it is reported, not
//! guessed at.
//!
//! What the recordings pinned (conformance/snapshots/test_logs.json, 2026-11-09): a term, two terms, a quoted phrase, an excluded
//! term, a JSON number comparison and a JSON string comparison select what they should, and `{ $.code = }` is an invalid pattern.
//! How a term is bounded in the message (a whole word, not a part of one) is from memory.

mod json;
mod space;

use serde_json::Value;

/// Why a pattern was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PatternError {
    /// AWS answers `Invalid filter pattern`.
    Invalid,
    /// A regular expression: not built.
    Regex,
}

/// A parsed pattern.
#[derive(Debug)]
pub(crate) enum Pattern {
    Any,
    Terms(Vec<Term>),
    Json(json::Expr),
    Space(space::Fields),
}

/// One term of an unstructured pattern.
#[derive(Debug)]
pub(crate) struct Term {
    text: String,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Must,
    Not,
    Either,
}

/// Parses `text`.
///
/// # Errors
/// [`PatternError::Invalid`] for a pattern that is not one, [`PatternError::Regex`] for a regular expression.
// fuzz:parser fuzz_filter_pattern
pub(crate) fn parse(text: &str) -> Result<Pattern, PatternError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Pattern::Any);
    }
    if text.starts_with('{') {
        return json::parse(text).map(Pattern::Json);
    }
    if text.starts_with('[') {
        return space::parse(text).map(Pattern::Space);
    }
    terms(text).map(Pattern::Terms)
}

impl Pattern {
    /// Whether the message is selected.
    pub(crate) fn matches(&self, message: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Terms(terms) => terms_match(terms, message),
            Self::Json(expr) => {
                serde_json::from_str::<Value>(message).is_ok_and(|v| expr.matches(&v))
            }
            Self::Space(fields) => fields.matches(message),
        }
    }

    /// The text a metric value or dimension (`$.latency`, `$bytes`) selects from a message that matched.
    pub(crate) fn pick(&self, message: &str, selector: &str) -> Option<String> {
        match self {
            Self::Json(_) | Self::Any | Self::Terms(_) => {
                let v: Value = serde_json::from_str(message).ok()?;
                json::Selector::parse(selector)?.first(&v).map(json::show)
            }
            Self::Space(fields) => fields.field(message, selector.trim_start_matches('$')),
        }
    }

    /// The values a space-delimited pattern names, for `TestMetricFilter`.
    pub(crate) fn extracted(&self, message: &str) -> Vec<(String, String)> {
        match self {
            Self::Space(fields) => fields.named(message),
            _ => Vec::new(),
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn terms(text: &str) -> Result<Vec<Term>, PatternError> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let kind = match c {
            '-' => Kind::Not,
            '?' => Kind::Either,
            _ => Kind::Must,
        };
        if kind != Kind::Must {
            chars.next();
        }
        let mut word = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            loop {
                match chars.next() {
                    Some('\\') => word.push(chars.next().ok_or(PatternError::Invalid)?),
                    Some('"') => break,
                    Some(ch) => word.push(ch),
                    None => return Err(PatternError::Invalid),
                }
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
        }
        if word.is_empty() {
            return Err(PatternError::Invalid);
        }
        if word.contains('%') {
            return Err(PatternError::Regex);
        }
        out.push(Term { text: word, kind });
    }
    Ok(out)
}

fn terms_match(terms: &[Term], message: &str) -> bool {
    let mut either = None;
    for t in terms {
        let found = contains_term(message, &t.text);
        match t.kind {
            Kind::Must if !found => return false,
            Kind::Not if found => return false,
            Kind::Either => either = Some(either.unwrap_or(false) || found),
            _ => {}
        }
    }
    either.unwrap_or(true)
}

/// Whether `term` occurs in `message` as a whole word (or phrase). A `*` stands for any run of characters that are not blanks.
fn contains_term(message: &str, term: &str) -> bool {
    if term.contains('*') {
        return message.split_whitespace().any(|token| glob(term, token));
    }
    let first_word = term.chars().next().is_some_and(is_word);
    let last_word = term.chars().last().is_some_and(is_word);
    message.match_indices(term).any(|(at, _)| {
        let before = message.get(..at).and_then(|s| s.chars().next_back());
        let after = message
            .get(at + term.len()..)
            .and_then(|s| s.chars().next());
        (!first_word || !before.is_some_and(is_word)) && (!last_word || !after.is_some_and(is_word))
    })
}

/// `*` matches any run of characters; everything else must match exactly.
pub(crate) fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = parts.split_first() else {
        return text.is_empty();
    };
    let Some(mut at) = text.strip_prefix(first).map(|r| text.len() - r.len()) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return at == text.len();
    };
    for part in middle {
        match text.get(at..).and_then(|t| t.find(part)) {
            Some(i) => at += i + part.len(),
            None => return false,
        }
    }
    text.get(at..).is_some_and(|t| t.ends_with(last)) && text.len() >= at + last.len()
}

#[cfg(test)]
mod tests;
