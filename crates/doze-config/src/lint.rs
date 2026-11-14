//! What loads but is likely a mistake (Go's `lintWarnings`; `doze lint` prints them): a variable
//! nothing reads, a task nothing waits for and not marked manual.

use crate::body::Block;
use crate::files::Files;
use crate::load::Runnable;
use crate::model::{Config, Kind};
use crate::refs::walk_body;
use doze_hcl::value::quote::go_quote;
use std::collections::BTreeSet;

pub(crate) fn warnings(
    files: &Files,
    cfg: &Config,
    variables: &[Block<'_>],
    runnables: &[Runnable<'_>],
) -> Vec<String> {
    let mut read = BTreeSet::new();
    for f in &files.files {
        let mut ts = Vec::new();
        walk_body(&f.body, &f.src, false, &mut ts);
        read.extend(
            ts.into_iter()
                .filter(|t| t.root == "var")
                .filter_map(|t| t.name().map(str::to_owned)),
        );
    }
    let mut out = Vec::new();
    for b in variables {
        let name = b.label(0);
        if !read.contains(name) {
            out.push(format!(
                "{}: variable {} is declared but nothing reads var.{name}",
                b.def,
                go_quote(name)
            ));
        }
    }
    let waited: BTreeSet<&str> = cfg
        .instances
        .iter()
        .flat_map(|d| d.deps.iter().map(|x| x.name.as_str()))
        .collect();
    for t in &cfg.tasks {
        if t.manual || waited.contains(t.name.as_str()) {
            continue;
        }
        let Some(r) = runnables
            .iter()
            .find(|r| r.kind == Kind::Task && r.name == t.name)
        else {
            continue;
        };
        let n = &t.name;
        out.push(format!(
            "{}: task {} runs only by doze task {n}: nothing lists it in after; add manual = true to say so, or task.{n} to the after of what needs it",
            r.def, go_quote(n)
        ));
    }
    out.sort();
    out
}
