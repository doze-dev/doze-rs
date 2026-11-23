//! What the console's GET addresses may call: a list, a detail, a row's columns, a related list and finding a resource by its
//! name are each made when a page is opened, so each must be a read. A write is only ever a POST or DELETE (behind the
//! same-origin proof), so no page opened from a link can change anything.

use super::ALL;
use crate::resources::{
    Batch, Blocker, Clear, Id, Kept, Kind, Options, Step, Tags, Write, deep, ops,
};

/// The beginnings of an operation that reads and changes nothing.
const READS: &[&str] = &["Get", "List", "Describe", "Head", "Scan", "Query", "Lookup"];

fn reads(step: &Step) -> bool {
    READS.iter().any(|r| step.op.starts_with(r))
}

#[test]
fn every_operation_a_get_makes_is_a_read() {
    let mut seen = 0;
    for d in ALL.iter().filter_map(|e| e.descriptor) {
        for k in d.kinds {
            let mut steps: Vec<&Step> = vec![&k.list.step];
            steps.extend(k.each.iter());
            steps.extend(k.detail.iter());
            steps.extend(k.subs.iter().map(|s| &s.step));
            if let Id::Call(step, _) = &k.id_of {
                steps.push(step);
            }
            // The addresses of tags, blockers and a list's selectors are GETs too.
            steps.extend(k.tags.iter().map(|t| &t.list));
            steps.extend(reading_blockers(k.blockers));
            steps.extend(k.kept.iter().filter_map(|kept| match kept {
                Kept::Read(step, _) => Some(step),
                Kept::Text(_) => None,
            }));
            steps.extend(k.params.iter().filter_map(|p| match &p.options {
                Options::Listed { step, .. } => Some(step),
                _ => None,
            }));
            for step in steps {
                seen += 1;
                assert!(
                    reads(step),
                    "{}/{}: a GET makes {}",
                    d.service,
                    k.id,
                    step.op
                );
            }
        }
    }
    assert!(seen > 50, "only {seen} steps were looked at");
}

/// The steps that list what a delete meets, and what hangs under each.
fn reading_blockers(blockers: &'static [Blocker]) -> Vec<&'static Step> {
    let mut out = Vec::new();
    for b in blockers {
        out.push(&b.list);
        if let Some(Clear::Cascade { under, .. }) = &b.clear {
            out.extend(reading_blockers(under));
        }
    }
    out
}

/// Every body a descriptor writes is JSON with placeholders.
fn bodies(k: &Kind) -> Vec<&'static str> {
    let mut out: Vec<&str> = vec![k.list.step.body];
    out.extend(k.each.iter().chain(k.detail).map(|s| s.body));
    out.extend(k.ops.iter().map(|o| o.bind));
    out.extend(k.deep.iter().flat_map(|d| d.ops.iter().map(|o| o.bind)));
    if let Some(Tags { list, write, .. }) = &k.tags {
        out.push(list.body);
        match write {
            Write::Read => {}
            Write::Delta { add, remove, .. } => out.extend([add.bind, remove.bind]),
            Write::Replace { put, clear, .. } => {
                out.push(put.bind);
                out.extend(clear.iter().map(|c| c.bind));
            }
        }
    }
    let mut stack: Vec<&Blocker> = k.blockers.iter().collect();
    while let Some(b) = stack.pop() {
        out.push(b.list.body);
        match &b.clear {
            Some(Clear::Each { step, .. } | Clear::Batch(Batch { step, .. })) => {
                out.push(step.body)
            }
            Some(Clear::Cascade { under, step, .. }) => {
                out.push(step.body);
                stack.extend(under.iter());
            }
            None => {}
        }
    }
    out
}

#[test]
fn every_body_is_json_and_every_verb_names_an_operation_its_pages_may_make() {
    for d in ALL.iter().filter_map(|e| e.descriptor) {
        for k in d.kinds {
            for body in bodies(k) {
                assert!(
                    serde_json::from_str::<serde_json::Value>(body).is_ok(),
                    "{}/{}: {body} is not JSON",
                    d.service,
                    k.id
                );
            }
            for v in k.page.verbs.iter().filter(|v| !v.op.is_empty()) {
                assert!(
                    deep::allowed(k, v.op).is_some(),
                    "{}/{}: the verb {} makes {}, which no page of the kind may",
                    d.service,
                    k.id,
                    v.id,
                    v.op
                );
            }
            // A kind's own operations are listed once.
            let mut seen = std::collections::BTreeSet::new();
            for o in k.ops {
                assert!(seen.insert(o.op), "{}/{}: {} twice", d.service, k.id, o.op);
                assert!(ops::allowed(k, o.op).is_some());
            }
        }
    }
}
