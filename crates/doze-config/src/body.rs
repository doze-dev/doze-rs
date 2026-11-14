//! A block's body as doze reads it: the body as written, with an `*.override.doze.hcl` override's
//! arguments laid over it (Go's `overrideBody`), and the blocks of a body with their headers.

use crate::error::At;
use doze_hcl::decode::{Attr, BlockRef, BodyRef, Schema};
use doze_hcl::diag::{Diagnostics, Range};
use std::collections::BTreeMap;

/// A body, maybe overridden.
#[derive(Debug, Clone)]
pub struct Body<'a> {
    base: BodyRef<'a>,
    over: Option<BodyRef<'a>>,
}

/// What a schema found in a body.
#[derive(Debug, Clone)]
pub struct Content<'a> {
    pub attributes: BTreeMap<String, Attr<'a>>,
    pub blocks: Vec<Block<'a>>,
}

impl<'a> Content<'a> {
    pub fn attr(&self, name: &str) -> Option<&Attr<'a>> {
        self.attributes.get(name)
    }
}

/// A block with its header.
#[derive(Debug, Clone)]
pub struct Block<'a> {
    pub ty: &'a str,
    pub labels: Vec<&'a str>,
    /// From the type through the last label (Go's `DefRange`).
    pub def: At,
    pub type_range: Range,
    pub body: Body<'a>,
}

impl<'a> Block<'a> {
    fn of(b: BlockRef<'a>) -> Block<'a> {
        let last = b.label_ranges.last().copied().unwrap_or(b.type_range);
        let def = At::new(b.body.file_name(), Range::between(b.type_range, last));
        Block {
            ty: b.ty,
            labels: b.labels,
            def,
            type_range: b.type_range,
            body: Body::new(b.body),
        }
    }

    pub fn label(&self, i: usize) -> &'a str {
        self.labels.get(i).copied().unwrap_or("")
    }

    /// `type label label`, the key an override is matched by.
    pub fn head_key(&self) -> String {
        std::iter::once(self.ty)
            .chain(self.labels.iter().copied())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn content_of(c: doze_hcl::decode::Content<'_>) -> Content<'_> {
    Content {
        attributes: c.attributes,
        blocks: c.blocks.into_iter().map(Block::of).collect(),
    }
}

impl<'a> Body<'a> {
    pub fn new(base: BodyRef<'a>) -> Body<'a> {
        Body { base, over: None }
    }

    /// This body with `over`'s arguments in place of its own.
    pub fn overridden(self, over: BodyRef<'a>) -> Body<'a> {
        Body {
            base: self.base,
            over: Some(over),
        }
    }

    pub fn file_name(&self) -> &str {
        self.base.file_name()
    }

    pub fn missing(&self) -> At {
        At::new(self.base.file_name(), self.base.missing_item_range())
    }

    pub fn content(&self, schema: &Schema) -> (Content<'a>, Diagnostics) {
        let (c, ds) = self.base.content(schema);
        let mut c = content_of(c);
        if let Some(over) = &self.over {
            let (oc, _, _) = over.partial_content(schema);
            c.attributes.extend(oc.attributes);
        }
        (c, ds)
    }

    pub fn partial_content(&self, schema: &Schema) -> (Content<'a>, Body<'a>, Diagnostics) {
        let (c, rest, ds) = self.base.partial_content(schema);
        let mut c = content_of(c);
        let over = self.over.as_ref().map(|over| {
            let (oc, orest, _) = over.partial_content(schema);
            c.attributes.extend(oc.attributes);
            orest
        });
        (c, Body { base: rest, over }, ds)
    }

    pub fn just_attributes(&self) -> (BTreeMap<String, Attr<'a>>, Diagnostics) {
        let (mut attrs, ds) = self.base.just_attributes();
        if let Some(over) = &self.over {
            attrs.extend(over.just_attributes().0);
        }
        (attrs, ds)
    }

    /// Every attribute expression under this body (nested blocks included), whatever a schema
    /// took, the base's then the override's: what references are read from.
    pub fn syntax_parts(
        &self,
    ) -> Vec<(&'a hcl_edit::structure::Body, &'a doze_hcl::syntax::Source)> {
        self.base
            .syntax()
            .chain(self.over.iter().flat_map(BodyRef::syntax))
            .map(|(b, s, _)| (b, s))
            .collect()
    }

    /// Only the parts as written, without the override (a module's own body: its decode).
    pub fn base(&self) -> &BodyRef<'a> {
        &self.base
    }

    pub fn over(&self) -> Option<&BodyRef<'a>> {
        self.over.as_ref()
    }
}

/// A schema of optional attributes.
pub(crate) fn attrs_schema(names: &[&str]) -> Schema {
    names
        .iter()
        .fold(Schema::default(), |s, n| s.attr(n, false))
}
