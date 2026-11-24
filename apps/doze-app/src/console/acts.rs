//! What can be done to a table's selected row, drawn in the table when no detail beside it shows that: in the
//! table's own actions column, never over the row's cells; and nothing offered on a table whose pick is gone.

use doze_desk::page::console::Node;
use gpui::{Div, SharedString, div, prelude::*, px};

use super::blocks::Draw;
use crate::instance::ui::text;
use crate::theme::ty;
use crate::widgets::kbd_chip;

/// The width of the actions column: room for each action's letter and label.
fn gutter(n: &Node) -> f32 {
    n.row_actions
        .iter()
        .map(|a| a.label.chars().count() as f32 * 7.0 + 44.)
        .sum::<f32>()
        + 6. * n.row_actions.len().saturating_sub(1) as f32
}

impl Draw<'_> {
    /// What can be done to the selected row is drawn in the table when no detail beside it shows that.
    pub(super) fn acts_in_table(&self, n: &Node) -> bool {
        self.live.is_some() && !n.detailed && !n.row_actions.is_empty()
    }

    /// The table's actions column, sized to the labels: every row has it, so nothing moves as the selection does,
    /// and in a narrow card the data columns give up width (their words cut short) before it does.
    pub(super) fn gutter(&self, n: &Node) -> Option<f32> {
        self.acts_in_table(n).then(|| gutter(n))
    }

    /// The selected row's actions: each its letter and its label, a click runs it (asleep, they wait, dimmed).
    pub(super) fn row_actions(&self, n: &Node) -> Div {
        let t = self.t;
        let mut d = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(self.asleep, |d| d.opacity(0.5));
        for (i, a) in n.row_actions.iter().enumerate() {
            let mut b = div()
                .id(SharedString::from(format!("{}-ra{i}", n.id)))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(24.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.line2)
                .bg(t.panel)
                .text_size(px(ty::SM))
                .text_color(if self.asleep {
                    t.dim
                } else if a.destructive {
                    t.bad
                } else {
                    t.text
                })
                .child(kbd_chip(t, &a.key.to_string(), ty::XS, ty::SEMIBOLD, 5.))
                .child(text(&a.label));
            if let Some(e) = self.live.as_ref().filter(|_| !self.asleep) {
                let (e, a) = (e.clone(), a.clone());
                b = b
                    .cursor_pointer()
                    .hover(|s| s.bg(t.sel))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        e.update(cx, |v, cx| v.act(a.clone(), cx));
                    });
            }
            d = d.child(b);
        }
        d
    }
}

/// Nothing picked in the table `id`: no row drawn as selected, and nothing offered on "the selected row" (its own
/// actions, and those of a detail showing that row).
pub(super) fn unpick(n: &mut doze_desk::page::console::Node, id: &str) {
    let row = n
        .walk()
        .into_iter()
        .find(|t| t.id == id)
        .and_then(|t| t.rows.iter().find(|r| r.at == t.selected))
        .map(|r| r.ident.clone());
    let Some(row) = row else {
        return;
    };
    fn clear(n: &mut doze_desk::page::console::Node, id: &str, row: &str) {
        if n.id == id {
            n.selected = usize::MAX;
            n.row_actions.clear();
        }
        if n.row_actions.iter().any(|a| a.row == row) {
            n.row_actions.clear();
            n.detail.clear();
        }
        for c in &mut n.children {
            clear(c, id, row);
        }
    }
    clear(n, id, &row);
}
