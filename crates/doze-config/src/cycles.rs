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
    let mut w = Walk {
        edges: &edges,
        state: BTreeMap::new(),
    };
    let names: Vec<&str> = edges.keys().copied().collect();
    for n in names {
        if let Some(cycle) = w.visit(n, &mut Vec::new()) {
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

struct Walk<'e> {
    edges: &'e BTreeMap<&'e str, Vec<&'e str>>,
    state: BTreeMap<&'e str, u8>,
}

impl<'e> Walk<'e> {
    /// The path to the first name seen twice, if there is one.
    fn visit(&mut self, n: &'e str, stack: &mut Vec<&'e str>) -> Option<Vec<&'e str>> {
        match self.state.get(n) {
            Some(2) => return None,
            Some(1) => {
                let mut c = stack.clone();
                c.push(n);
                return Some(c);
            }
            _ => {}
        }
        self.state.insert(n, 1);
        stack.push(n);
        for m in self.edges.get(n).into_iter().flatten() {
            if let Some(c) = self.visit(m, stack) {
                return Some(c);
            }
        }
        stack.pop();
        self.state.insert(n, 2);
        None
    }
}
