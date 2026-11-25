//! A console tree from the keyboard, once a person has clicked in it (it is the focused block): ↑↓ move the cursor,
//! → opens a folder or steps in, ← closes one or steps out, ⏎ opens a leaf in the value viewer. The cursor's row (or
//! the picked leaf) is kept in view in the tree's own scroll.

use doze_desk::page::tree::{Fetch, TreeState};
use gpui::Context;

use super::ConsoleTabView;

impl ConsoleTabView {
    /// The focused tree, if the focused block is one.
    fn focused_tree(&self) -> Option<String> {
        self.focused
            .clone()
            .filter(|f| self.work.trees.contains_key(f))
    }

    /// Does `f` on the focused tree and keeps its cursor in view; `false` when no tree is focused.
    pub(super) fn tree_keys(
        &mut self,
        f: impl FnOnce(&mut TreeState) -> Option<Fetch>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tree) = self.focused_tree() else {
            return false;
        };
        let fetch = self.work.trees.get_mut(&tree).and_then(|s| f(&mut s.state));
        if let Some(fetch) = fetch {
            self.fetch_children(&tree, fetch, cx);
        }
        self.reveal_in_tree(&tree);
        cx.notify();
        true
    }

    pub(super) fn tree_right(&mut self, cx: &mut Context<Self>) {
        self.tree_keys(TreeState::right, cx);
    }

    pub(super) fn tree_left(&mut self, cx: &mut Context<Self>) {
        self.tree_keys(
            |t| {
                t.left();
                None
            },
            cx,
        );
    }

    pub(super) fn tree_enter(&mut self, cx: &mut Context<Self>) {
        let mut opened = false;
        self.tree_keys(
            |t| {
                let (f, leaf) = t.enter();
                opened = leaf;
                f
            },
            cx,
        );
        if opened {
            self.sync_work(cx);
        }
    }

    /// Scrolls tree `tree` so its cursor (or picked leaf) shows.
    pub(crate) fn reveal_in_tree(&self, tree: &str) {
        if let Some(s) = self.work.trees.get(tree)
            && let Some(i) = s.state.cursor_row()
        {
            s.scroll.scroll_to_item(i);
        }
    }
}
