//! `*.override.doze.hcl`: files that change `port`, `enabled`, `sleep` or `version` on blocks
//! declared elsewhere (Go's `overrides.go`).

use crate::body::Block;
use crate::error::{At, Result, pos_err};
use crate::files::{File, Files};
use crate::kinds::is_kind;
use doze_hcl::decode::BodyRef;
use doze_hcl::diag::Range;
use hcl_edit::Span;
use std::collections::BTreeMap;

pub(crate) const SUFFIX: &str = ".override.doze.hcl";
const OVERRIDABLE: [&str; 4] = ["port", "enabled", "sleep", "version"];

fn range(f: &File, span: Option<std::ops::Range<usize>>) -> Range {
    span.map(|s| f.src.range(s)).unwrap_or_default()
}

/// A block body's range from its `{` through its `}` (hclsyntax's `Body.SrcRange`).
fn braces(
    f: &File,
    head_end: Option<std::ops::Range<usize>>,
    block: Option<std::ops::Range<usize>>,
) -> Range {
    let text = f.src.text();
    let from = head_end.map_or(0, |h| h.end);
    let open = text
        .get(from..)
        .and_then(|t| t.find('{'))
        .map_or(from, |i| from + i);
    let end = block.map_or(open, |b| {
        let upto = text.get(..b.end).unwrap_or("");
        upto.rfind('}').map_or(b.end, |i| i + 1)
    });
    f.src.range(open..end.max(open))
}

/// An override's body, by the head it changes (`module postgres db`), with where it is.
pub(crate) type Overrides<'a> = BTreeMap<String, (BodyRef<'a>, At)>;

/// Go's `splitOverrides`: the files that declare, and the overrides, checked.
pub(crate) fn split<'a>(files: &'a Files) -> Result<(Vec<&'a File>, Overrides<'a>)> {
    let mut base = Vec::new();
    let mut ov = Overrides::new();
    for f in &files.files {
        let is_override = std::path::Path::new(&f.name)
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(SUFFIX));
        if !is_override {
            base.push(f);
            continue;
        }
        if let Some(a) = f.body.attributes().next() {
            return Err(pos_err(
                files,
                &At::new(&f.name, range(f, a.key.span())),
                format!(
                    "an override file changes blocks; {} is a stack-wide setting",
                    a.key.as_str()
                ),
                "set it in doze.hcl, or move it into a block",
            ));
        }
        for b in f.body.blocks() {
            let last = b.labels.last().and_then(|l| l.span()).or(b.ident.span());
            let def = At::new(
                &f.name,
                Range::between(range(f, b.ident.span()), range(f, last.clone())),
            );
            check_block(files, f, b, &def)?;
            let key = std::iter::once(b.ident.as_str())
                .chain(b.labels.iter().map(|l| l.as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            if ov.contains_key(&key) {
                return Err(pos_err(
                    files,
                    &def,
                    "this block is overridden twice",
                    "keep one override per block",
                ));
            }
            let whole = At::new(&f.name, braces(f, last, b.span()));
            ov.insert(key, (BodyRef::file(&b.body, &f.src, &f.name), whole));
        }
    }
    Ok((base, ov))
}

fn check_block(files: &Files, f: &File, b: &hcl_edit::structure::Block, def: &At) -> Result<()> {
    if !is_kind(b.ident.as_str()) {
        return Err(pos_err(
            files,
            def,
            format!(
                "an override file changes module, app, service and task blocks, not {}",
                b.ident.as_str()
            ),
            "",
        ));
    }
    if let Some(inner) = b.body.blocks().next() {
        let last = inner
            .labels
            .last()
            .and_then(|l| l.span())
            .or(inner.ident.span());
        let at = At::new(
            &f.name,
            Range::between(range(f, inner.ident.span()), range(f, last)),
        );
        return Err(pos_err(
            files,
            &at,
            format!(
                "an override changes {}; not nested blocks",
                OVERRIDABLE.join(", ")
            ),
            "change it in the block where it is declared",
        ));
    }
    if let Some(a) = b
        .body
        .attributes()
        .find(|a| !OVERRIDABLE.contains(&a.key.as_str()))
    {
        let n = a.key.as_str();
        return Err(pos_err(
            files,
            &At::new(&f.name, range(f, a.key.span())),
            format!("an override changes {}; not {n}", OVERRIDABLE.join(", ")),
            format!("change {n} in the block where it is declared"),
        ));
    }
    Ok(())
}

/// Go's `overrides.apply`: each override laid over its block; one that matches nothing refused.
pub(crate) fn apply<'a>(files: &Files, ov: &Overrides<'a>, blocks: &mut [Block<'a>]) -> Result<()> {
    let mut used = std::collections::BTreeSet::new();
    for b in blocks.iter_mut() {
        let key = b.head_key();
        if let Some((o, _)) = ov.get(&key) {
            b.body = b.body.clone().overridden(o.clone());
            used.insert(key);
        }
    }
    if let Some((k, (_, at))) = ov.iter().find(|(k, _)| !used.contains(*k)) {
        return Err(pos_err(
            files,
            at,
            format!("the override for {k} matches nothing declared"),
            "an override changes a block declared in doze.hcl; check the kind, type and name match",
        ));
    }
    Ok(())
}
