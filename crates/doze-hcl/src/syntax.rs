//! The syntax layer: hcl-edit parses, and a `Source` turns its byte spans into Go-style positions.

use crate::diag::{Diagnostic, Pos, Range};
use hcl_edit::structure::Body;
use unicode_segmentation::UnicodeSegmentation;

/// A source text and where its lines start, to turn byte offsets into positions.
#[derive(Debug, Clone)]
pub struct Source {
    text: String,
    line_starts: Vec<usize>,
}

impl Source {
    pub fn new(text: &str) -> Source {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Source {
            text: text.to_owned(),
            line_starts,
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
        let column = self
            .text
            .get(start..byte)
            .map_or(byte - start, |s| s.graphemes(true).count())
            + 1;
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

/// Parses a whole file body. A syntax error comes back as one diagnostic at its position.
// fuzz:parser fuzz_parse
pub fn parse_body(src: &Source) -> Result<Body, Diagnostic> {
    hcl_edit::parser::parse_body(src.text()).map_err(|e| {
        let p = src.pos(e.location().offset());
        Diagnostic::error(
            "Syntax error",
            e.message().to_string(),
            Range { start: p, end: p },
        )
    })
}
