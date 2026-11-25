//! What a console's working blocks (module protocol minor 2) resolve to: a value viewer knows what it shows (the row
//! picked in the table it follows, or the leaf opened in a tree), a tree knows its panel and whether a value block
//! on its tab opens its leaves. The value itself and a folder's children are asked of the daemon as a person looks
//! (the hub's `value` and `children`); nothing here reads the module.

use super::console::{Node, block_id, row_ident, wire_walk};
use super::resolve::{Resolver, default_title};
use doze_control as wire;
use doze_display::CELL_RUNTIME;

/// A cell the hub adds to a row (a rate it worked out), not one the module sent.
fn hub_cell(key: &str) -> bool {
    key.starts_with("_rate:") || key == CELL_RUNTIME
}

impl Resolver<'_> {
    /// The block `from` names on this tab: by its id, else the table or tree of that panel.
    fn followed(&self, from: &str) -> Option<&wire::ConsoleNode> {
        if from.is_empty() {
            return None;
        }
        let nodes = wire_walk(self.body);
        nodes
            .iter()
            .find(|b| block_id(b) == from)
            .or_else(|| {
                nodes
                    .iter()
                    .find(|b| b.panel == from && matches!(b.kind.as_str(), "table" | "tree"))
            })
            .copied()
    }

    pub(super) fn value(&self, out: &mut Node, n: &wire::ConsoleNode) {
        default_title(out, "value");
        out.panel.clone_from(&n.panel);
        let Some(t) = self.followed(&n.from) else {
            out.missing = if n.from.is_empty() {
                "This value follows nothing: the module named no table or tree for it.".to_owned()
            } else {
                format!("This value follows “{}”, which is not on this tab.", n.from)
            };
            return;
        };
        out.follows = block_id(t);
        if out.panel.is_empty() {
            out.panel.clone_from(&t.panel);
        }
        if t.kind == "tree" {
            out.follows_tree = true;
            return;
        }
        let (Some(rows), Some((_, p))) = (self.c.tables.get(&t.panel), self.panel(&t.panel)) else {
            return;
        };
        if rows.is_empty() {
            return;
        }
        let i = self
            .sel
            .get(&out.follows)
            .copied()
            .unwrap_or(0)
            .min(rows.len() - 1);
        let Some(row) = rows.get(i) else { return };
        out.row = row
            .cells
            .iter()
            .filter(|(k, _)| !hub_cell(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.pick = format!("{}\u{1f}{}", t.panel, row_ident(rows, i, p.first_col()));
        out.pick_name = row.cell(p.first_col()).to_owned();
    }

    pub(super) fn tree(&self, out: &mut Node, n: &wire::ConsoleNode) {
        default_title(out, "tree");
        if n.panel.is_empty() {
            out.missing = "This tree has no panel to ask its folders of.".to_owned();
            return;
        }
        if let Some((p, _)) = self.panel(&n.panel)
            && out.title == "tree"
        {
            p.title.clone_into(&mut out.title);
        }
        out.panel.clone_from(&n.panel);
        out.paired = wire_walk(self.body).iter().any(|v| {
            v.kind == "value" && !v.from.is_empty() && (v.from == out.id || v.from == n.panel)
        });
    }
}
