//! What can be done to a table's selected row, drawn in the table when no detail beside it shows that: in a long
//! table's actions column (its rows are a list of one height), in a short one on a line under the row. Never over
//! the row's cells.

use doze_desk::page::console::Node;
use gpui::{Div, SharedString, div, prelude::*, px};

use super::blocks::Draw;
use super::parts::{BIG, ROW_H};
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

    /// A long table's actions column (its rows are a list of one height, so the actions cannot have a line of their
    /// own); a short table has them on a line under the selected row instead (`action_line_under`).
    pub(super) fn gutter(&self, n: &Node) -> Option<f32> {
        (self.acts_in_table(n) && n.total > BIG).then(|| gutter(n))
    }

    /// Under a short table's selected row: what can be done to it, on a line of its own.
    pub(super) fn action_line_under(&self, n: &Node) -> Div {
        let t = self.t;
        div()
            .flex()
            .justify_end()
            .px(px(14.))
            .h(px(ROW_H))
            .items_center()
            .bg(if self.focused.as_deref() == Some(n.id.as_str()) {
                t.pick
            } else {
                t.sel
            })
            .child(self.row_actions(n))
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
                .text_color(if a.destructive { t.bad } else { t.text })
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
