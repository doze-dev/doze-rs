//! `doze status --graph`: the dependency tree instead of the table (Go `renderGraph`). Roots are the services nothing
//! needs; under each, what it needs, sorted, drawn with box lines.

use crate::cmd::status::{
    Report, awake, endpoint_text, engine_cell, glyph, human_bytes, state_cell,
};
use crate::paint::Paint;
use doze_control::InstanceView;
use std::collections::{BTreeMap, BTreeSet};

impl Report {
    /// The tree, and the line under it.
    pub(crate) fn graph(&self, p: Paint) -> String {
        let mut needs: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut needed: BTreeSet<&str> = BTreeSet::new();
        for v in &self.views {
            for n in &v.needs {
                needs.entry(&v.name).or_default().push(n);
                needed.insert(n);
            }
        }
        let mut roots: Vec<&str> = self
            .views
            .iter()
            .map(|v| v.name.as_str())
            .filter(|n| !needed.contains(n))
            .collect();
        roots.sort_unstable();
        let mut out = String::new();
        if !self.running {
            if self.views.is_empty() {
                return format!("{}\n", p.dim("doze is not running"));
            }
            out.push_str(&p.dim("doze is not running — showing declared services"));
            out.push_str("\n\n");
        }
        let tree = Tree {
            report: self,
            needs: &needs,
            p,
        };
        for r in &roots {
            tree.node(&mut out, r, "", Pos::Root, 0);
        }
        if self.running {
            out.push('\n');
            out.push_str(&p.dim(&self.summary("")));
            out.push('\n');
        }
        out
    }
}

/// Where a node sits: a root, or the last or a middle child.
#[derive(Clone, Copy)]
enum Pos {
    Root,
    Last,
    Middle,
}

struct Tree<'a> {
    report: &'a Report,
    needs: &'a BTreeMap<&'a str, Vec<&'a str>>,
    p: Paint,
}

impl Tree<'_> {
    fn node(&self, out: &mut String, name: &str, prefix: &str, pos: Pos, depth: usize) {
        let connector = match pos {
            Pos::Root => "",
            Pos::Last => "└─ ",
            Pos::Middle => "├─ ",
        };
        out.push_str(prefix);
        out.push_str(connector);
        out.push_str(&self.label(name));
        out.push('\n');
        // A cycle the daemon would refuse is still never drawn for ever.
        if depth > self.report.views.len() {
            return;
        }
        let child = match pos {
            Pos::Root => prefix.to_owned(),
            Pos::Last => format!("{prefix}   "),
            Pos::Middle => format!("{prefix}│  "),
        };
        let mut deps = self.needs.get(name).cloned().unwrap_or_default();
        deps.sort_unstable();
        let end = deps.len().saturating_sub(1);
        for (i, d) in deps.iter().enumerate() {
            let pos = if i == end { Pos::Last } else { Pos::Middle };
            self.node(out, d, &child, pos, depth + 1);
        }
    }

    /// One node: glyph, name, engine, state, endpoint, and a compact tail of what it uses when awake.
    fn label(&self, name: &str) -> String {
        let p = self.p;
        let Some(v) = self.report.views.iter().find(|v| v.name == name) else {
            return format!("{} {}", p.dim("·"), p.bold(name));
        };
        let mut line = format!("{} {}", glyph(v, p), p.bold(&v.name));
        line.push(' ');
        line.push_str(&p.dim(&engine_cell(v)));
        line.push_str("  ");
        line.push_str(&state_cell(v, p));
        if let Some(ep) = endpoint_text(v) {
            line.push_str("  ");
            line.push_str(&ep);
        }
        let tail = meters(v);
        if !tail.is_empty() {
            line.push_str(&p.dim(&format!("  {tail}")));
        }
        line
    }
}

/// "3c · 11M · 2%" for an awake instance.
fn meters(v: &InstanceView) -> String {
    if !awake(&v.state) {
        return String::new();
    }
    let mut m = Vec::new();
    if v.conns > 0 {
        m.push(format!("{}c", v.conns));
    }
    if v.ram > 0 {
        m.push(human_bytes(v.ram));
    }
    if v.cpu >= 0.5 {
        m.push(format!("{:.0}%", v.cpu));
    }
    m.join(" · ")
}
