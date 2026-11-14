//! The syntax layer: hcl-edit parses, and a `Source` turns its byte spans into Go-style positions.

use crate::diag::{Diagnostic, Diagnostics, Pos, Range};
use hcl_edit::structure::Body;

/// A source text and where its lines start, to turn byte offsets into positions.
#[derive(Debug, Clone)]
pub struct Source {
    text: String,
    line_starts: Vec<usize>,
    /// Per line: whether it is all ASCII (then a column is a byte count).
    ascii: Vec<bool>,
}

impl Source {
    pub fn new(text: &str) -> Source {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let ascii = text.split('\n').map(str::is_ascii).collect();
        Source {
            text: text.to_owned(),
            line_starts,
            ascii,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The position of a byte offset (clamped to the text). Columns count grapheme clusters from
    /// the line start, as Go's scanner does.
    pub fn pos(&self, byte: usize) -> Pos {
        let byte = byte.min(self.text.len());
        let line = self.line_starts.partition_point(|&s| s <= byte);
        let start = line
            .checked_sub(1)
            .and_then(|i| self.line_starts.get(i))
            .copied()
            .unwrap_or(0);
        // A byte order mark is skipped as Go skips it: its bytes count, its column does not.
        let start = if line == 1 && self.text.starts_with('\u{feff}') {
            start.max(3).min(byte)
        } else {
            start
        };
        let ascii = line
            .checked_sub(1)
            .and_then(|i| self.ascii.get(i))
            .copied()
            .unwrap_or(true);
        let column = if ascii {
            byte - start
        } else {
            self.text.get(start..byte).map_or(byte - start, columns)
        } + 1;
        let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        Pos {
            line: n(line),
            column: n(column),
            byte: n(byte),
        }
    }

    pub fn range(&self, span: std::ops::Range<usize>) -> Range {
        Range {
            start: self.pos(span.start),
            end: self.pos(span.end),
        }
    }
}

/// Parses a whole file body. A file Go refuses comes back with every diagnostic Go gives, in Go's
/// order and at Go's positions (`check`); hcl-edit builds the tree only for a file that passes, so
/// it never sees broken nesting.
// fuzz:parser fuzz_parse
pub fn parse_body(src: &Source) -> Result<Body, Diagnostics> {
    let found = crate::check::check(src.text().as_bytes(), "x.hcl");
    if found.has_errors() {
        return Err(found);
    }
    // Go skips a leading byte order mark; hcl-edit refuses it, so it reads blanks in its place
    // (the same byte offsets).
    let text = match src.text().strip_prefix('\u{feff}') {
        Some(rest) => std::borrow::Cow::Owned(format!("   {rest}")),
        None => std::borrow::Cow::Borrowed(src.text()),
    };
    hcl_edit::parser::parse_body(&text).map_err(|e| {
        // Only hcl-edit's own limits get here (catalogue S5): Go accepts the file.
        let p = src.pos(e.location().offset());
        Diagnostics::from(Diagnostic::error(
            "Syntax error",
            e.message().to_string(),
            Range { start: p, end: p },
        ))
    })
}

/// Columns in a line prefix as Go counts them: grapheme clusters, segmented token by token. A
/// cluster that starts with ASCII punctuation or a space and carries more characters (a combining
/// mark, a joiner) is two in Go, because the punctuation is a token of its own.
fn columns(s: &str) -> usize {
    crate::funcs::graphemes::clusters(s)
        .into_iter()
        .map(|(_, g)| g)
        .map(|g| {
            let mut cs = g.chars();
            let first = cs
                .next()
                .is_some_and(|c| c.is_ascii_punctuation() || c == ' ' || c == '\t');
            if first && cs.next().is_some() { 2 } else { 1 }
        })
        .sum()
}
