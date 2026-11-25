//! A console tree from the keyboard: ↑↓ move the cursor over the rows drawn, → opens the folder at it (or steps into
//! one already open), ← closes it (or steps out to its parent), ⏎ opens a leaf in the value viewer or opens or closes
//! a folder. The cursor is a node's id, so it stays on its node as pages come in above it.

use super::tree::{Fetch, TreeRow, TreeState};

/// A row's node: its id, depth, whether it is a leaf and open.
fn node(r: &TreeRow) -> Option<(&str, usize, bool, bool)> {
    match r {
        TreeRow::Node {
            id,
            depth,
            leaf,
            open,
            ..
        } => Some((id.as_str(), *depth, *leaf, *open)),
        _ => None,
    }
}

impl TreeState {
    /// Where the cursor is among the rows drawn (what to scroll into view), else the picked leaf's place.
    pub fn cursor_row(&self) -> Option<usize> {
        let at = self.cursor.as_deref().or(self.picked.as_deref())?;
        self.rows()
            .iter()
            .position(|r| node(r).is_some_and(|(id, ..)| id == at))
    }

    /// ↑ or ↓: the cursor moves `by` nodes (from the picked leaf, or the first node, when it is on none).
    pub fn step(&mut self, by: isize) {
        let rows = self.rows();
        let ids: Vec<&str> = rows.iter().filter_map(node).map(|(id, ..)| id).collect();
        if ids.is_empty() {
            return;
        }
        let now = self.cursor.as_deref().or(self.picked.as_deref());
        let next = match now.and_then(|c| ids.iter().position(|id| *id == c)) {
            Some(i) => (i as isize + by).clamp(0, ids.len() as isize - 1) as usize,
            None => 0,
        };
        self.cursor = ids.get(next).map(|id| (*id).to_owned());
    }

    /// →: opens the folder at the cursor (asked for when not read yet), or steps onto its first child when open.
    pub fn right(&mut self) -> Option<Fetch> {
        let rows = self.rows();
        let at = self.cursor_row()?;
        let (id, depth, leaf, open) = rows.get(at).and_then(node)?;
        if leaf {
            return None;
        }
        if !open {
            let id = id.to_owned();
            return self.open_one(&id);
        }
        let child = rows
            .get(at + 1)
            .and_then(node)
            .filter(|(_, d, ..)| *d == depth + 1)
            .map(|(c, ..)| c.to_owned());
        if child.is_some() {
            self.cursor = child;
        }
        None
    }

    /// ←: closes the folder at the cursor, or steps out to its parent.
    pub fn left(&mut self) {
        let rows = self.rows();
        let Some(at) = self.cursor_row() else { return };
        let Some((id, depth, leaf, open)) = rows.get(at).and_then(node) else {
            return;
        };
        if !leaf && open {
            self.open.remove(id);
            self.cursor = Some(id.to_owned());
            return;
        }
        let parent = rows
            .iter()
            .take(at)
            .rev()
            .filter_map(node)
            .find(|(_, d, ..)| *d < depth)
            .map(|(p, ..)| p.to_owned());
        if parent.is_some() {
            self.cursor = parent;
        }
    }

    /// ⏎: the leaf at the cursor opens in the value viewer (`true`); a folder opens or closes.
    pub fn enter(&mut self) -> (Option<Fetch>, bool) {
        let rows = self.rows();
        let Some((id, _, leaf, _)) = self.cursor_row().and_then(|at| rows.get(at)).and_then(node)
        else {
            return (None, false);
        };
        let id = id.to_owned();
        if leaf {
            self.picked = Some(id);
            return (None, true);
        }
        (self.toggle(&id), false)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use doze_control::{ChildrenView, TreeNodeView};

    fn n(id: &str, leaf: bool) -> TreeNodeView {
        TreeNodeView {
            id: id.into(),
            label: id.into(),
            leaf,
            ..TreeNodeView::default()
        }
    }

    fn page(nodes: Vec<TreeNodeView>) -> ChildrenView {
        ChildrenView {
            nodes,
            next: String::new(),
        }
    }

    #[test]
    fn the_keys_walk_open_and_close_the_tree() {
        let mut t = TreeState::default();
        let top = t.start().unwrap();
        t.got(&top, page(vec![n("a", false), n("b", true)]));
        t.step(1);
        assert_eq!(
            t.cursor.as_deref(),
            Some("a"),
            "first key lands on the first row"
        );
        let f = t.right().unwrap();
        assert_eq!(f.parent, "a", "→ opens a folder not read yet");
        t.got(&f, page(vec![n("a/x", true), n("a/y", true)]));
        assert_eq!(t.right(), None);
        assert_eq!(
            t.cursor.as_deref(),
            Some("a/x"),
            "→ on an open folder steps in"
        );
        t.step(1);
        assert_eq!(t.enter(), (None, true));
        assert_eq!(t.picked.as_deref(), Some("a/y"), "⏎ opens the leaf");
        t.left();
        assert_eq!(
            t.cursor.as_deref(),
            Some("a"),
            "← on a leaf goes to its parent"
        );
        t.left();
        assert!(!t.open.contains("a"), "← on an open folder closes it");
        t.step(5);
        assert_eq!(
            t.cursor.as_deref(),
            Some("b"),
            "the cursor stops at the end"
        );
        assert_eq!(t.cursor_row(), Some(1));
    }
}
