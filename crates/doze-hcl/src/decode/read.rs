//! Reading one body against a schema: hcl's `PartialContent` and `Content`'s refusals, with
//! hcl's diagnostics at hcl's ranges.

use super::{Attr, BlockRef, BlockSchema, BodyRef, Content, Part, Schema};
use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::eval::suggestion;
use crate::syntax::Source;
use hcl_edit::Span;
use hcl_edit::structure::{Attribute, Block, BlockLabel, Body};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn empty_content<'a>() -> Content<'a> {
    Content {
        attributes: BTreeMap::new(),
        blocks: Vec::new(),
        missing: Range::default(),
    }
}

pub(super) fn part<'a>(body: &'a Body, src: &'a Source, file: &'a str, missing: Range) -> Part<'a> {
    Part {
        body,
        src,
        file,
        missing,
        hidden_attrs: BTreeSet::new(),
        hidden_blocks: BTreeSet::new(),
    }
}

pub(super) fn duplicate(first: &Attr<'_>, again: &Attr<'_>) -> Diagnostic {
    let detail = format!(
        "Argument {:?} was already set at {}",
        again.name,
        crate::check::place(first.file, first.name_range)
    );
    Diagnostic::error("Duplicate argument", detail, again.name_range).in_file(again.file)
}

/// Where the peeker's first token is: Go's top-level body starts there.
pub(super) fn first_token(src: &Source) -> Option<usize> {
    crate::lex::lex(src.text().as_bytes())
        .into_iter()
        .find(|t| {
            t.kind != crate::lex::Kind::Comment || t.bytes(src.text().as_bytes()).ends_with(b"\n")
        })
        .map(|t| t.start)
}

pub(super) fn ident_range(p: &Part<'_>, b: &Block) -> Range {
    b.ident.span().map(|s| p.src.range(s)).unwrap_or_default()
}

pub(super) fn attr_of<'a>(p: &Part<'a>, a: &'a Attribute) -> Attr<'a> {
    Attr {
        name: a.key.as_str(),
        expr: &a.value,
        src: p.src,
        file: p.file,
        name_range: a.key.span().map(|s| p.src.range(s)).unwrap_or_default(),
        range: a.span().map(|s| p.src.range(s)).unwrap_or_default(),
    }
}

/// `PartialContent` of one body.
pub(super) fn partial<'a>(p: &Part<'a>, schema: &Schema) -> (Content<'a>, Part<'a>, Diagnostics) {
    let mut rest = p.clone();
    let mut ds = Diagnostics::default();
    let mut attributes = BTreeMap::new();
    for s in &schema.attributes {
        let found = p
            .body
            .get_attribute(&s.name)
            .filter(|_| !p.hidden_attrs.contains(&s.name));
        let Some(a) = found else {
            if s.required {
                let detail = format!(
                    "The argument {:?} is required, but no definition was found.",
                    s.name
                );
                ds.push(Diagnostic::error(
                    "Missing required argument",
                    detail,
                    p.missing,
                ));
            }
            continue;
        };
        rest.hidden_attrs.insert(s.name.clone());
        attributes.insert(s.name.clone(), attr_of(p, a));
    }
    let mut blocks = Vec::new();
    for b in p
        .body
        .blocks()
        .filter(|b| !p.hidden_blocks.contains(b.ident.as_str()))
    {
        let Some(s) = schema.blocks.iter().find(|s| s.ty == b.ident.as_str()) else {
            continue;
        };
        match block_of(p, b, s) {
            Ok(block) => blocks.push(block),
            Err(d) => ds.push(d.in_file(p.file)),
        }
    }
    rest.hidden_blocks
        .extend(schema.blocks.iter().map(|s| s.ty.clone()));
    (
        Content {
            attributes,
            blocks,
            missing: p.missing,
        },
        rest,
        tag(ds, p.file),
    )
}

pub(super) fn block_of<'a>(
    p: &Part<'a>,
    b: &'a Block,
    s: &BlockSchema,
) -> Result<BlockRef<'a>, Box<Diagnostic>> {
    let type_range = ident_range(p, b);
    let label_ranges: Vec<Range> = b
        .labels
        .iter()
        .map(|l| l.span().map(|sp| p.src.range(sp)).unwrap_or_default())
        .collect();
    let open = open_brace(p, b);
    let ctx = Range {
        start: type_range.start,
        end: open.end,
    };
    let name = b.ident.as_str();
    if b.labels.len() > s.labels.len() {
        let detail = if s.labels.is_empty() {
            format!("No labels are expected for {name} blocks.")
        } else {
            format!(
                "Only {} labels ({}) are expected for {name} blocks.",
                s.labels.len(),
                s.labels.join(", ")
            )
        };
        let at = label_ranges
            .get(s.labels.len())
            .copied()
            .unwrap_or(type_range);
        return Err(Box::new(
            Diagnostic::error(format!("Extraneous label for {name}"), detail, at).with_context(ctx),
        ));
    }
    if let Some(missing) = s.labels.get(b.labels.len()) {
        let detail = format!(
            "All {name} blocks must have {} labels ({}).",
            s.labels.len(),
            s.labels.join(", ")
        );
        return Err(Box::new(
            Diagnostic::error(format!("Missing {missing} for {name}"), detail, open)
                .with_context(ctx),
        ));
    }
    let labels = b.labels.iter().map(label_str).collect();
    let missing = body_start(p, b, open);
    Ok(BlockRef {
        ty: name,
        labels,
        type_range,
        label_ranges,
        open_brace: open,
        body: BodyRef {
            parts: vec![part(&b.body, p.src, p.file, missing)],
            merged: false,
        },
    })
}

pub(super) fn label_str(l: &BlockLabel) -> &str {
    match l {
        BlockLabel::Ident(i) => i.as_str(),
        BlockLabel::String(s) => s.as_str(),
    }
}

/// The block's `{`: the first after its header.
pub(super) fn open_brace(p: &Part<'_>, b: &Block) -> Range {
    let span = b.span().unwrap_or_default();
    let from = b
        .labels
        .last()
        .and_then(Span::span)
        .or_else(|| b.ident.span())
        .map_or(span.start, |s| s.end);
    let at = p
        .src
        .text()
        .get(from..span.end)
        .and_then(|t| t.find('{'))
        .map_or(from, |i| from + i);
    p.src.range(at..at + 1)
}

/// Where a missing item in this block's body is reported: the `{`, or for a one-line block
/// (`b { x = 1 }`) its attribute, as Go's single-attribute body has the attribute's range.
pub(super) fn body_start(p: &Part<'_>, b: &Block, open: Range) -> Range {
    let one_line = b
        .body
        .attributes()
        .next()
        .filter(|_| b.body.blocks().next().is_none())
        .and_then(|a| {
            let s = a.span()?;
            (p.src.range(s.clone()).start.line == open.start.line)
                .then(|| p.src.range(s.start..s.start))
        });
    one_line.unwrap_or(Range {
        start: open.start,
        end: open.start,
    })
}

/// `Content`'s refusals: attributes and blocks the schema did not take.
pub(super) fn unsupported(rest: &Part<'_>, schema: &Schema, content: &Content<'_>) -> Diagnostics {
    let mut ds = Diagnostics::default();
    for a in rest
        .body
        .attributes()
        .filter(|a| !rest.hidden_attrs.contains(a.key.as_str()))
    {
        let name = a.key.as_str();
        let candidates: Vec<String> = schema
            .attributes
            .iter()
            .filter(|s| !content.attributes.contains_key(&s.name))
            .map(|s| s.name.clone())
            .collect();
        let hint = match suggestion(name, &candidates) {
            Some(s) => format!(" Did you mean {s:?}?"),
            None if schema.blocks.iter().any(|b| b.ty == name) => {
                format!(" Did you mean to define a block of type {name:?}?")
            }
            None => String::new(),
        };
        let at = a.key.span().map(|s| rest.src.range(s)).unwrap_or_default();
        ds.push(Diagnostic::error(
            "Unsupported argument",
            format!("An argument named {name:?} is not expected here.{hint}"),
            at,
        ));
    }
    for b in rest
        .body
        .blocks()
        .filter(|b| !rest.hidden_blocks.contains(b.ident.as_str()))
    {
        let ty = b.ident.as_str();
        let candidates: Vec<String> = schema.blocks.iter().map(|s| s.ty.clone()).collect();
        let hint = match suggestion(ty, &candidates) {
            Some(s) => format!(" Did you mean {s:?}?"),
            None if schema.attributes.iter().any(|a| a.name == ty) => format!(
                " Did you mean to define argument {ty:?}? If so, use the equals sign to assign it a value."
            ),
            None => String::new(),
        };
        ds.push(Diagnostic::error(
            "Unsupported block type",
            format!("Blocks of type {ty:?} are not expected here.{hint}"),
            ident_range(rest, b),
        ));
    }
    tag(ds, rest.file)
}

/// Names the file on the diagnostics that do not name one yet.
pub(super) fn tag(ds: Diagnostics, file: &str) -> Diagnostics {
    Diagnostics(
        ds.0.into_iter()
            .map(|d| if d.file.is_none() { d.in_file(file) } else { d })
            .collect(),
    )
}
