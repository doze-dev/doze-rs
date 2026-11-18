//! A stack whose blocks wait on each other in a circle is refused (Go's `checkCycles`).

use crate::error::{At, Result, Sources, pos_err};
use crate::model::Config;
use std::collections::BTreeMap;

pub(crate) fn check(cfg: &Config, src: &dyn Sources, ranges: &BTreeMap<String, At>) -> Result<()> {
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for d in &cfg.instances {
        let e = edges.entry(d.name.as_str()).or_default();
        e.extend(d.deps.iter().map(|x| x.name.as_str()));
        e.extend(d.refs.iter().chain(&d.after).map(|r| r.name.as_str()));
    }
    for s in &cfg.services {
        edges
            .entry(s.name.as_str())
            .or_default()
            .extend(s.refs.iter().chain(&s.after).map(|r| r.name.as_str()));
    }
    for t in &cfg.tasks {
        edges
            .entry(t.name.as_str())
            .or_default()
            .extend(t.refs.iter().chain(&t.after).map(|r| r.name.as_str()));
    }
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    let names: Vec<&str> = edges.keys().copied().collect();
    for n in names {
        if let Some(cycle) = walk(&edges, &mut state, n) {
            let at = cycle
                .last()
                .and_then(|n| ranges.get(*n))
                .cloned()
                .unwrap_or_default();
            return Err(pos_err(src, &at, "dependency cycle", cycle.join(" → ")));
        }
    }
    Ok(())
}

/// Go's recursive `visit`, with a stack of its own: the path to the first name seen twice.
fn walk<'e>(
    edges: &BTreeMap<&'e str, Vec<&'e str>>,
    state: &mut BTreeMap<&'e str, u8>,
    root: &'e str,
) -> Option<Vec<&'e str>> {
    if state.get(root) == Some(&2) {
        return None;
    }
    state.insert(root, 1);
    let mut path: Vec<(&'e str, usize)> = vec![(root, 0)];
    while let Some((n, next)) = path.last_mut() {
        let n = *n;
        let Some(&m) = edges.get(n).and_then(|e| e.get(*next)) else {
            path.pop();
            state.insert(n, 2);
            continue;
        };
        *next += 1;
        match state.get(m) {
            Some(2) => {}
            Some(1) => {
                let mut c: Vec<&str> = path.iter().map(|(k, _)| *k).collect();
                c.push(m);
                return Some(c);
            }
            _ => {
                state.insert(m, 1);
                path.push((m, 0));
            }
        }
    }
    None
}
