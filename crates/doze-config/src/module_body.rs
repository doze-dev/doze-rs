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
