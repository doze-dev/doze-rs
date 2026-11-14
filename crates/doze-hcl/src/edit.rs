//! Editing a config file the way hclwrite does for `doze add` and `doze init`: the file is a token
//! sequence; an edit appends tokens (a newline, a formatted block, an attribute) before the end of
//! file, and writing formats the whole sequence. Comments, order and everything not touched stay.

use crate::diag::Diagnostics;
use crate::fmt::{Tok, format, render, writer_tokens};
use crate::lex::{Kind, lex};
use crate::syntax::{Source, parse_body};
use hcl_edit::Span;
use hcl_edit::structure::Body;

/// A token the edit owns.
#[derive(Debug, Clone)]
struct Owned {
    kind: Kind,
    bytes: Vec<u8>,
    spaces: usize,
}

#[derive(Debug, Clone)]
pub struct Edit {
    tokens: Vec<Owned>,
    /// The end-of-file token, with the spaces before it (kept, as Go keeps them).
    eof: Owned,
    body: Body,
    src: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The snippet does not parse.
    Snippet(Diagnostics),
    /// The snippet is not exactly one block.
    NotOneBlock(usize),
}

fn owned(src: &[u8]) -> Vec<Owned> {
    let tokens = lex(src);
    writer_tokens(src, &tokens)
        .into_iter()
        .map(|t| Owned {
            kind: t.kind,
            bytes: t.bytes.to_vec(),
            spaces: t.spaces,
        })
        .collect()
}

impl Edit {
    /// Opens a file for editing; a file that does not parse is refused with its diagnostics.
    pub fn open(src: &str) -> Result<Edit, Diagnostics> {
        let body = parse_body(&Source::new(src))?;
        let mut tokens = owned(src.as_bytes());
        let eof = tokens.pop().unwrap_or(Owned {
            kind: Kind::Eof,
            bytes: Vec::new(),
            spaces: 0,
        });
        Ok(Edit {
            tokens,
            eof,
            body,
            src: src.to_owned(),
        })
    }

    /// The top-level blocks: type and labels, in order. As in hclwrite, a quoted label whose text the
    /// scanner splits into more than one literal token is left out.
    pub fn blocks(&self) -> Vec<(String, Vec<String>)> {
        self.body
            .blocks()
            .map(|b| {
                let labels = b
                    .labels
                    .iter()
                    .filter(|l| self.single_token(l))
                    .map(|l| l.as_str().to_owned())
                    .collect();
                (b.ident.as_str().to_owned(), labels)
            })
            .collect()
    }

    pub fn has_attribute(&self, name: &str) -> bool {
        self.body.get_attribute(name).is_some()
    }

    fn has_items(&self) -> bool {
        !self.body.is_empty()
    }

    /// Appends one block, formatted, after a blank line when the file has items (`Edit.AppendBlock`).
    pub fn append_block(&mut self, block: &str) -> Result<(), EditError> {
        let text = String::from_utf8_lossy(&format(block.as_bytes())).into_owned();
        let snippet = parse_body(&Source::new(&text)).map_err(EditError::Snippet)?;
        let blocks: Vec<_> = snippet.blocks().collect();
        let [only] = blocks.as_slice() else {
            return Err(EditError::NotOneBlock(blocks.len()));
        };
        let span = only.span().unwrap_or_default();
        let mut toks = owned(text.as_bytes());
        toks.pop();
        // The block's tokens, through the newline that ends it.
        let taken = block_tokens(toks, &span);
        if self.has_items() {
            self.push_newline();
        }
        self.tokens.extend(taken);
        Ok(())
    }

    fn push_newline(&mut self) {
        self.tokens.push(Owned {
            kind: Kind::Newline,
            bytes: b"\n".to_vec(),
            spaces: 0,
        });
    }

    /// Sets `name = "value"` at the top of the file, unless the file sets it already
    /// (`Edit.SetStack`): appended to a file with no items, otherwise first, then a blank line.
    pub fn set_top_attribute(&mut self, name: &str, value: &str) {
        if self.has_attribute(name) {
            return;
        }
        let line = format!("{name} = \"{}\"\n", quote(value));
        if !self.has_items() {
            let mut toks = owned(line.as_bytes());
            toks.pop();
            self.tokens.extend(toks);
            return;
        }
        let old = String::from_utf8_lossy(&self.write_raw()).into_owned();
        if let Ok(e) = Edit::open(&format!("{line}\n{old}")) {
            *self = e;
        }
    }

    fn write_raw(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for t in self.tokens.iter().chain(std::iter::once(&self.eof)) {
            out.resize(out.len() + t.spaces, b' ');
            out.extend_from_slice(&t.bytes);
        }
        out
    }

    /// The edited file: formatted as hclwrite writes it, ending in exactly one newline.
    pub fn bytes(&self) -> Vec<u8> {
        let toks: Vec<Tok<'_>> = self
            .tokens
            .iter()
            .chain(std::iter::once(&self.eof))
            .map(|t| Tok {
                kind: t.kind,
                bytes: &t.bytes,
                spaces: t.spaces,
            })
            .collect();
        let mut out = render(toks);
        while out.last() == Some(&b'\n') {
            out.pop();
        }
        out.push(b'\n');
        out
    }
}

/// hclwrite's `escapeQuotedStringLit`.
fn quote(s: &str) -> String {
    // cty strings are NFC.
    let s: String = unicode_normalization::UnicodeNormalization::nfc(s).collect();
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' | '%' => {
                out.push(c);
                if chars.peek() == Some(&'{') {
                    out.push(c);
                }
            }
            c if !crate::value::quote::is_go_printable(c) => {
                let n = u32::from(c);
                if n < 0x10000 {
                    out.push_str(&format!("\\u{n:04x}"));
                } else {
                    out.push_str(&format!("\\U{n:08x}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

impl Edit {
    /// Whether a label is one token in Go's scanner (an identifier, or a quoted string the scanner
    /// does not split at a `$` or `%`): hclwrite reads only those as labels.
    fn single_token(&self, l: &hcl_edit::structure::BlockLabel) -> bool {
        let Some(span) = l.span() else { return true };
        let raw = self.src.get(span).unwrap_or_default();
        let literals = lex(raw.as_bytes())
            .iter()
            .filter(|t| t.kind == Kind::QuotedLit)
            .count();
        literals <= 1
    }
}

/// A snippet block's tokens as hclwrite takes them: the comments right before it (no blank line
/// between), the block, and the newline that ends it.
fn block_tokens(toks: Vec<Owned>, span: &std::ops::Range<usize>) -> Vec<Owned> {
    let mut at = 0;
    let starts: Vec<usize> = toks
        .iter()
        .map(|t| {
            let start = at + t.spaces;
            at = start + t.bytes.len();
            start
        })
        .collect();
    let mut first = starts
        .iter()
        .position(|s| *s >= span.start)
        .unwrap_or(toks.len());
    while first > 0 && toks.get(first - 1).is_some_and(|t| t.kind == Kind::Comment) {
        first -= 1;
    }
    let mut taken = Vec::new();
    for (t, start) in toks.into_iter().zip(starts).skip(first) {
        let newline = start >= span.end && t.kind == Kind::Newline;
        taken.push(t);
        if newline {
            break;
        }
    }
    taken
}
