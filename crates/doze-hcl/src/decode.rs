//! Bodies against a schema (hcl's `Body.Content`, `PartialContent`, `JustAttributes` and
//! `MergeFiles`): which attributes and blocks a body has, the ones it lacks or should not have, with
//! hcl's diagnostics at hcl's ranges. What a caller decodes the attributes into is its own business
//! (`value` converts one expression to a type, as gohcl's `DecodeExpression` does).

use crate::diag::{Diagnostic, Diagnostics, Range};
use crate::eval::{Context, Evaluator};
use crate::syntax::Source;
use crate::types::Type;
use crate::value::Value;
use hcl_edit::Span;
use hcl_edit::expr::Expression;
use hcl_edit::structure::Body;
use std::collections::{BTreeMap, BTreeSet};

mod read;

use read::{
    attr_of, duplicate, empty_content, first_token, ident_range, part, partial, unsupported,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrSchema {
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSchema {
    pub ty: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    pub attributes: Vec<AttrSchema>,
    pub blocks: Vec<BlockSchema>,
}

impl Schema {
    pub fn attr(mut self, name: &str, required: bool) -> Schema {
        self.attributes.push(AttrSchema {
            name: name.to_owned(),
            required,
        });
        self
    }

    pub fn block(mut self, ty: &str, labels: &[&str]) -> Schema {
        self.blocks.push(BlockSchema {
            ty: ty.to_owned(),
            labels: labels.iter().map(|l| (*l).to_owned()).collect(),
        });
        self
    }
}

/// One file's body, or a block's: the syntax, its source and name, and what an earlier
/// `partial_content` already took.
#[derive(Debug, Clone)]
struct Part<'a> {
    body: &'a Body,
    src: &'a Source,
    file: &'a str,
    /// Where an item that is missing is reported: the start of the body.
    missing: Range,
    hidden_attrs: BTreeSet<String>,
    hidden_blocks: BTreeSet<String>,
}

/// A body to read: one block's, one file's, or several files merged (`hcl.MergeFiles`).
#[derive(Debug, Clone)]
pub struct BodyRef<'a> {
    parts: Vec<Part<'a>>,
    merged: bool,
}

/// An attribute found in a body.
#[derive(Debug, Clone)]
pub struct Attr<'a> {
    pub name: &'a str,
    pub expr: &'a Expression,
    pub src: &'a Source,
    pub file: &'a str,
    pub name_range: Range,
    pub range: Range,
}

/// A block found in a body.
#[derive(Debug, Clone)]
pub struct BlockRef<'a> {
    pub ty: &'a str,
    pub labels: Vec<&'a str>,
    pub type_range: Range,
    pub label_ranges: Vec<Range>,
    pub open_brace: Range,
    pub body: BodyRef<'a>,
}

#[derive(Debug, Clone)]
pub struct Content<'a> {
    pub attributes: BTreeMap<String, Attr<'a>>,
    pub blocks: Vec<BlockRef<'a>>,
    pub missing: Range,
}

impl<'a> BodyRef<'a> {
    /// A whole file's body.
    pub fn file(body: &'a Body, src: &'a Source, file: &'a str) -> BodyRef<'a> {
        let start = first_token(src).unwrap_or_default();
        let at = src.range(start..start);
        BodyRef {
            parts: vec![part(body, src, file, at)],
            merged: false,
        }
    }

    /// Several files read as one (`hcl.MergeFiles`): an attribute may be set in one file only;
    /// blocks are concatenated in file order.
    pub fn merge(files: Vec<BodyRef<'a>>) -> BodyRef<'a> {
        BodyRef {
            parts: files.into_iter().flat_map(|f| f.parts).collect(),
            merged: true,
        }
    }

    /// The syntax under this body, part by part, with its source and file name: everything in it,
    /// whatever `partial_content` took (Go's `hclsyntax.Body.Attributes` keeps those too).
    pub fn syntax(&self) -> impl Iterator<Item = (&'a Body, &'a Source, &'a str)> + '_ {
        self.parts.iter().map(|p| (p.body, p.src, p.file))
    }

    /// Whether `partial_content` already took the attribute `name` from this body.
    pub fn took(&self, name: &str) -> bool {
        self.parts.iter().all(|p| p.hidden_attrs.contains(name))
    }

    /// The file the missing-item range is in.
    pub fn file_name(&self) -> &str {
        self.parts.first().map_or("", |p| p.file)
    }

    pub fn missing_item_range(&self) -> Range {
        self.parts.first().map(|p| p.missing).unwrap_or_default()
    }

    /// `Content`: everything in the body must be in the schema.
    pub fn content(&self, schema: &Schema) -> (Content<'a>, Diagnostics) {
        if self.merged {
            let (c, _, d) = self.merged_content(schema, false);
            return (c, d);
        }
        let Some(p) = self.parts.first() else {
            return (empty_content(), Diagnostics::default());
        };
        let (content, rest, mut ds) = partial(p, schema);
        ds.extend(unsupported(&rest, schema, &content));
        (content, ds)
    }

    /// `PartialContent`: what the schema names, and the rest of the body for another reader.
    pub fn partial_content(&self, schema: &Schema) -> (Content<'a>, BodyRef<'a>, Diagnostics) {
        if self.merged {
            return self.merged_content(schema, true);
        }
        let Some(p) = self.parts.first() else {
            return (empty_content(), self.clone(), Diagnostics::default());
        };
        let (content, rest, ds) = partial(p, schema);
        (
            content,
            BodyRef {
                parts: vec![rest],
                merged: false,
            },
            ds,
        )
    }

    /// `JustAttributes`: a body of attributes only (blocks are refused).
    pub fn just_attributes(&self) -> (BTreeMap<String, Attr<'a>>, Diagnostics) {
        let mut out: BTreeMap<String, Attr<'a>> = BTreeMap::new();
        let mut ds = Diagnostics::default();
        for p in &self.parts {
            if let Some(b) = p.body.blocks().next() {
                let at = ident_range(p, b);
                ds.push(
                    Diagnostic::error(
                        format!("Unexpected {:?} block", b.ident.as_str()),
                        "Blocks are not allowed here.",
                        at,
                    )
                    .in_file(p.file),
                );
            }
            for a in p
                .body
                .attributes()
                .filter(|a| !p.hidden_attrs.contains(a.key.as_str()))
            {
                let attr = attr_of(p, a);
                match out.get(attr.name) {
                    Some(first) if self.merged => ds.push(duplicate(first, &attr)),
                    _ => {
                        out.insert(attr.name.to_owned(), attr);
                    }
                }
            }
        }
        (out, ds)
    }

    fn merged_content(
        &self,
        schema: &Schema,
        is_partial: bool,
    ) -> (Content<'a>, BodyRef<'a>, Diagnostics) {
        let mut each = schema.clone();
        for a in &mut each.attributes {
            a.required = false;
        }
        let mut content = Content {
            attributes: BTreeMap::new(),
            blocks: Vec::new(),
            missing: self.missing_item_range(),
        };
        let mut rest = Vec::new();
        let mut ds = Diagnostics::default();
        for p in &self.parts {
            let (c, r, d) = partial(p, &each);
            ds.extend(d);
            if !is_partial {
                ds.extend(unsupported(&r, &each, &c));
            }
            rest.push(r);
            for (name, attr) in c.attributes {
                match content.attributes.get(&name) {
                    Some(first) => ds.push(duplicate(first, &attr)),
                    None => {
                        content.attributes.insert(name, attr);
                    }
                }
            }
            content.blocks.extend(c.blocks);
        }
        for a in schema.attributes.iter().filter(|a| a.required) {
            if !content.attributes.contains_key(&a.name) {
                ds.push(Diagnostic {
                    severity: crate::diag::Severity::Error,
                    summary: "Missing required argument".into(),
                    detail: format!("The argument {:?} is required, but was not set.", a.name),
                    subject: None,
                    context: None,
                    file: None,
                });
            }
        }
        (
            content,
            BodyRef {
                parts: rest,
                merged: true,
            },
            ds,
        )
    }
}

/// gohcl's `DecodeExpression` into a Go value of type `ty`: the expression's own diagnostics, then
/// "Unsuitable value type" at the expression's start when the value does not convert, is not
/// known (`value must be known`), or is null where the Go target cannot hold null (`nullable`
/// false: a non-pointer field). The value is the converted one, or the dynamic unknown.
pub fn value(
    attr: &Attr<'_>,
    ctx: &Context<'_>,
    ty: &Type,
    nullable: bool,
) -> (Value, Diagnostics) {
    let ev = Evaluator::new(attr.src);
    let (v, ds) = ev.eval(attr.expr, ctx);
    let mut ds = Diagnostics(ds.0.into_iter().map(|d| d.in_file(attr.file)).collect());
    let whole = attr
        .expr
        .span()
        .map(|s| attr.src.range(s))
        .unwrap_or_default();
    let unsuitable = |why: String| {
        Diagnostic::error(
            "Unsuitable value type",
            format!("Unsuitable value: {why}"),
            ev.start_range(attr.expr),
        )
        .with_context(whole)
        .in_file(attr.file)
    };
    let v = match crate::convert::convert(&v, ty) {
        Ok(v) => v,
        Err(e) => {
            ds.push(unsuitable(e.0));
            return (Value::DYNAMIC, ds);
        }
    };
    if !v.is_wholly_known() {
        ds.push(unsuitable("value must be known".into()));
        return (Value::DYNAMIC, ds);
    }
    if v.is_null() && !nullable {
        ds.push(unsuitable("null value is not allowed".into()));
        return (Value::DYNAMIC, ds);
    }
    (v, ds)
}
