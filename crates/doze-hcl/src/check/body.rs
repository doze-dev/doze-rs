//! Bodies, attributes and blocks (`ParseBody`, `ParseBodyItem`, `parseSingleAttrBody`,
//! `finishParsingBodyAttribute`, `finishParsingBodyBlock`, `parseQuotedStringLiteral`).

use super::{Ds, Parser, between, err, place};
use crate::diag::Range;
use crate::lex::{Kind, Token};
use std::collections::HashMap;

const ITEM_REQUIRED: &str = "An argument or block definition is required here.";
const UNCLOSED_BLOCK: &str = "There is no closing brace for this block before the end of the file. This may be caused by incorrect brace nesting elsewhere in this file.";

/// What a body item turned out to be.
enum Item {
    Attr { name: Vec<u8>, at: Range },
    Block,
    Nothing,
}

impl Parser<'_> {
    pub(crate) fn body(&mut self, end: Kind) -> Ds {
        let mut ds = Ds::new();
        if !self.enter(&mut ds) {
            self.recover(end);
            return ds;
        }
        let start = self.prev_range();
        let mut attrs: HashMap<Vec<u8>, Range> = HashMap::new();
        loop {
            let next = self.peek();
            if next.kind == end {
                self.read();
                break;
            }
            match next.kind {
                Kind::Newline => {
                    self.read();
                }
                Kind::Ident => {
                    let (item, d) = self.body_item();
                    ds.extend(d);
                    if let Item::Attr { name, at } = item {
                        if let Some(first) = attrs.get(&name) {
                            let detail = format!(
                                "The argument {:?} was already set at {}. Each argument may be set only once.",
                                String::from_utf8_lossy(&name),
                                place(self.file, *first)
                            );
                            ds.push(err("Attribute redefined", detail, at));
                        } else {
                            attrs.insert(name, at);
                        }
                    }
                }
                _ => {
                    let bad = self.read();
                    if !self.recovery {
                        ds.push(self.bad_body_token(&bad, end, start));
                    }
                    self.recover(end);
                    break;
                }
            }
        }
        self.leave();
        ds
    }

    fn bad_body_token(&self, bad: &Token, end: Kind, start: Range) -> crate::diag::Diagnostic {
        match bad.kind {
            Kind::OQuote => err(
                "Invalid argument name",
                "Argument names must not be quoted.",
                bad.range,
            ),
            Kind::Eof if end == Kind::CBrace => {
                err("Unclosed configuration block", UNCLOSED_BLOCK, start)
            }
            Kind::Eof => err(
                "Unclosed configuration body",
                "Found end of file before the end of this configuration body.",
                start,
            ),
            _ => err(
                "Argument or block definition required",
                ITEM_REQUIRED,
                bad.range,
            ),
        }
    }

    fn body_item(&mut self) -> (Item, Ds) {
        let ident = self.read();
        let next = self.peek();
        match next.kind {
            Kind::Equal => self.attribute(ident, false),
            Kind::OQuote | Kind::OBrace | Kind::Ident => (Item::Block, self.block(ident)),
            _ => {
                self.recover_after_body_item();
                let detail = format!(
                    "{ITEM_REQUIRED} To set an argument, use the equals sign \"=\" to introduce the argument value."
                );
                (
                    Item::Nothing,
                    vec![err(
                        "Argument or block definition required",
                        detail,
                        ident.range,
                    )],
                )
            }
        }
    }

    fn single_attr_body(&mut self) -> Ds {
        let ident = self.read();
        if ident.kind != Kind::Ident {
            self.recover_after_body_item();
            return vec![err(
                "Argument or block definition required",
                ITEM_REQUIRED,
                ident.range,
            )];
        }
        let next = self.peek();
        match next.kind {
            Kind::Equal => self.attribute(ident, true).1,
            Kind::OQuote | Kind::OBrace | Kind::Ident => {
                self.recover_after_body_item();
                let detail = format!(
                    "A single-line block definition can contain only a single argument. If you meant to define argument {:?}, use an equals sign to assign it a value. To define a nested block, place it on a line of its own within its parent block.",
                    String::from_utf8_lossy(self.bytes(&ident))
                );
                vec![err(
                    "Argument definition required",
                    detail,
                    between(ident.range, next.range),
                )]
            }
            _ => {
                self.recover_after_body_item();
                let detail = format!(
                    "{ITEM_REQUIRED} To set an argument, use the equals sign \"=\" to introduce the argument value."
                );
                vec![err(
                    "Argument or block definition required",
                    detail,
                    ident.range,
                )]
            }
        }
    }

    fn attribute(&mut self, ident: Token, single_line: bool) -> (Item, Ds) {
        self.read(); // the equals sign
        let (_, mut ds) = self.expression();
        let errors = ds
            .iter()
            .any(|d| d.severity == crate::diag::Severity::Error);
        if self.recovery && errors {
            self.recover_after_body_item();
        } else if !single_line {
            let end = self.peek();
            if end.kind != Kind::Newline && end.kind != Kind::Eof {
                if !self.recovery {
                    let (summary, detail) = if end.kind == Kind::Comma {
                        (
                            "Unexpected comma after argument",
                            "Argument definitions must be separated by newlines, not commas. An argument definition must end with a newline.",
                        )
                    } else {
                        (
                            "Missing newline after argument",
                            "An argument definition must end with a newline.",
                        )
                    };
                    ds.push(
                        err(summary, detail, end.range)
                            .with_context(between(ident.range, end.range)),
                    );
                }
                self.recover_after_body_item();
            } else {
                self.read();
            }
        }
        let name = self.bytes(&ident).to_vec();
        (
            Item::Attr {
                name,
                at: ident.range,
            },
            ds,
        )
    }

    fn block(&mut self, ident: Token) -> Ds {
        let mut ds = Ds::new();
        let open = loop {
            let tok = self.peek();
            match tok.kind {
                Kind::OBrace => break self.read(),
                Kind::OQuote => ds.extend(self.quoted_string_literal()),
                Kind::Ident => {
                    self.read();
                }
                _ => {
                    let detail = match tok.kind {
                        Kind::Equal => Some(
                            "The equals sign \"=\" indicates an argument definition, and must not be used when defining a block.",
                        ),
                        Kind::Newline => Some(
                            "A block definition must have block content delimited by \"{\" and \"}\", starting on the same line as the block header.",
                        ),
                        _ if self.recovery => None,
                        _ => Some(
                            "Either a quoted string block label or an opening brace (\"{\") is expected here.",
                        ),
                    };
                    if let Some(detail) = detail {
                        ds.push(
                            err("Invalid block definition", detail, tok.range)
                                .with_context(between(ident.range, tok.range)),
                        );
                    }
                    self.recover_after_body_item();
                    return ds;
                }
            }
        };
        let body_ds = match self.peek().kind {
            Kind::Newline | Kind::Eof | Kind::CBrace => self.body(Kind::CBrace),
            _ => {
                let d = self.single_attr_body();
                self.close_single_line(ident, open, &mut ds);
                d
            }
        };
        ds.extend(body_ds);
        let eol = self.peek();
        if eol.kind == Kind::Newline || eol.kind == Kind::Eof {
            self.read();
        } else {
            if !self.recovery {
                ds.push(
                    err(
                        "Missing newline after block definition",
                        "A block definition must end with a newline.",
                        eol.range,
                    )
                    .with_context(between(ident.range, eol.range)),
                );
            }
            self.recover_after_body_item();
        }
        ds
    }

    fn close_single_line(&mut self, ident: Token, open: Token, ds: &mut Ds) {
        let next = self.peek();
        let summary = "Invalid single-argument block definition";
        match next.kind {
            Kind::CBrace => {
                self.read();
                return;
            }
            Kind::Comma => ds.push(err(summary, "Single-line block syntax can include only one argument definition. To define multiple arguments, use the multi-line block syntax with one argument definition per line.", next.range)),
            Kind::Newline => ds.push(err(summary, "An argument definition on the same line as its containing block creates a single-line block definition, which must also be closed on the same line. Place the block's closing brace immediately after the argument definition.", next.range)),
            _ if self.recovery => {}
            Kind::Eof => ds.push(
                err("Unclosed configuration block", UNCLOSED_BLOCK, open.range).with_context(between(ident.range, open.range)),
            ),
            _ => ds.push(err(summary, "A single-line block definition must end with a closing brace immediately after its single argument definition.", next.range)),
        }
        self.recover(Kind::CBrace);
    }

    /// A block label: a quoted string without template sequences.
    pub(crate) fn quoted_string_literal(&mut self) -> Ds {
        let open = self.read();
        if open.kind != Kind::OQuote {
            return vec![err(
                "Invalid string literal",
                "A quoted string is required here.",
                open.range,
            )];
        }
        let mut ds = Ds::new();
        loop {
            let tok = self.read();
            match tok.kind {
                Kind::CQuote => break,
                Kind::QuotedLit => {
                    ds.extend(super::strlit::escapes(self.bytes(&tok), tok.range, true))
                }
                Kind::TemplateControl | Kind::TemplateInterp => {
                    let which = if tok.kind == Kind::TemplateControl {
                        "%"
                    } else {
                        "$"
                    };
                    let detail = format!(
                        "Template sequences are not allowed in this string. To include a literal {which:?}, double it (as \"{which}{which}\") to escape it."
                    );
                    ds.push(
                        err("Invalid string literal", detail, tok.range)
                            .with_context(between(open.range, tok.range)),
                    );
                    self.recover(Kind::TemplateSeqEnd);
                }
                Kind::Eof => {
                    ds.push(
                        err(
                            "Unterminated string literal",
                            "Unable to find the closing quote mark before the end of the file.",
                            tok.range,
                        )
                        .with_context(between(open.range, tok.range)),
                    );
                    break;
                }
                _ => {
                    ds.push(
                        err(
                            "Invalid string literal",
                            "This item is not valid in a string literal.",
                            tok.range,
                        )
                        .with_context(between(open.range, tok.range)),
                    );
                    self.recover(Kind::CQuote);
                    break;
                }
            }
        }
        ds
    }
}
