//! A module block's own body, evaluated as written: the arguments doze did not take (in name
//! order) and the nested blocks (in order). This is what a module reads its config from.

use crate::body::Body;
use crate::eval::eval_expr;
use crate::model::{ModuleBlock, ModuleBody};
use doze_hcl::diag::Diagnostics;
use doze_hcl::eval::Context;
use doze_hcl::syntax::Source;
use hcl_edit::structure::Body as Syntax;

/// The body after the arguments every block has were taken.
pub(crate) fn decode(body: &Body<'_>, ctx: &Context<'_>) -> (ModuleBody, Diagnostics) {
    let mut out = ModuleBody::default();
    let mut ds = Diagnostics::default();
    let base = body.base();
    for (syntax, src, file) in base.syntax() {
        let (b, d) = read(syntax, src, file, ctx, &|n| base.took(n));
        out.attrs.extend(b.attrs);
        out.blocks.extend(b.blocks);
        ds.extend(d);
    }
    (out, ds)
}

fn read(
    syntax: &Syntax,
    src: &Source,
    file: &str,
    ctx: &Context<'_>,
    took: &dyn Fn(&str) -> bool,
) -> (ModuleBody, Diagnostics) {
    let mut out = ModuleBody::default();
    let mut ds = Diagnostics::default();
    let mut attrs: Vec<_> = syntax
        .attributes()
        .filter(|a| !took(a.key.as_str()))
        .collect();
    attrs.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
    for a in attrs {
        let (v, d) = eval_expr(&a.value, src, file, ctx);
        let failed = d.has_errors();
        ds.extend(d);
        if !failed {
            out.attrs.insert(a.key.as_str().to_owned(), v);
        }
    }
    for b in syntax.blocks() {
        let (inner, d) = read(&b.body, src, file, ctx, &|_| false);
        ds.extend(d);
        out.blocks.push(ModuleBlock {
            ty: b.ident.as_str().to_owned(),
            labels: b.labels.iter().map(|l| l.as_str().to_owned()).collect(),
            body: inner,
        });
    }
    (out, ds)
}

/// The shape pass's print of a module block's own body, for its fingerprint (the shape pass has no module to decode
/// it for): each argument doze did not take, in name order, as it evaluates (the modules it reads are their stand-ins
/// there) or, where it does not, as written; then the nested blocks, in order. An edit inside the block (a database,
/// a role, a setting) changes it, as Go's fingerprint of the full load saw it.
pub(crate) fn print(body: &Body<'_>, ctx: &Context<'_>) -> String {
    let mut out = String::new();
    let base = body.base();
    for (syntax, src, file) in base.syntax() {
        print_into(&mut out, syntax, src, file, ctx, &|n| base.took(n));
    }
    out
}

fn print_into(
    out: &mut String,
    syntax: &Syntax,
    src: &Source,
    file: &str,
    ctx: &Context<'_>,
    took: &dyn Fn(&str) -> bool,
) {
    use hcl_edit::Span as _;
    use std::fmt::Write as _;
    let mut attrs: Vec<_> = syntax
        .attributes()
        .filter(|a| !took(a.key.as_str()))
        .collect();
    attrs.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
    for a in attrs {
        let (v, d) = eval_expr(&a.value, src, file, ctx);
        if d.has_errors() {
            let written = a
                .value
                .span()
                .and_then(|sp| src.text().get(sp))
                .unwrap_or("");
            let _ = write!(out, "{}~{written:?};", a.key.as_str());
        } else {
            let _ = write!(out, "{}={v:?};", a.key.as_str());
        }
    }
    for b in syntax.blocks() {
        let labels: Vec<&str> = b.labels.iter().map(|l| l.as_str()).collect();
        let _ = write!(out, "{}{labels:?}{{", b.ident.as_str());
        print_into(out, &b.body, src, file, ctx, &|_| false);
        out.push('}');
    }
}
