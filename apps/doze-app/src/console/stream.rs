//! A console's streams: rows as they arrive, the newest kept in view (see `ConsoleTabView::refresh`); one with no
//! lines yet is a single line saying so, not a block taking the room left.

use doze_desk::page::console::Node;
use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::blocks::{Draw, share};
use super::parts::{LINE_H, MONO_CH, widths};
use crate::instance::ui::heading;
use crate::theme::ty;
use crate::widgets::Mono as _;

impl Draw<'_> {
    pub(super) fn stream(&self, n: &Node) -> AnyElement {
        let t = *self.t;
        let widths = widths(n, MONO_CH);
        let title = div()
            .px(px(16.))
            .pt(px(4.))
            .min_h(px(34.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(heading(&t, &n.title, None));
        let body: AnyElement = if n.rows.is_empty() {
            div()
                .px(px(16.))
                .pb(px(14.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child("Waiting for the first line.")
                .into_any_element()
        } else {
            self.lines(n, &widths)
        };
        let empty = n.rows.is_empty();
        let frame = if self.flat {
            share(div(), n).flex().flex_col().overflow_hidden()
        } else {
            share(div(), n)
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(t.line)
                .bg(t.panel)
                .shadow(t.lift())
                .overflow_hidden()
        };
        frame
            .when(!empty, |d| d.min_h(px(LINE_H * 6.)))
            .child(title)
            .when(!empty, |d| d.child(self.header(n, &widths, false)))
            .child(body)
            .into_any_element()
    }

    /// A stream's lines, the newest kept in view.
    fn lines(&self, n: &Node, widths: &[Option<f32>]) -> AnyElement {
        let t = *self.t;
        let (node, rows, widths) = (n.clone(), n.rows.clone(), widths.to_vec());
        let list = uniform_list(
            SharedString::from(format!("stream-{}", n.id)),
            rows.len(),
            move |range, _, _| {
                let d = Draw {
                    t: &t,
                    focused: None,
                    live: None,
                    stream: None,
                    lists: None,
                    filter: None,
                    flat: false,
                };
                range
                    .filter_map(|i| rows.get(i))
                    .map(|r| {
                        d.cells(&node, &widths, d.row_cells(r))
                            .h(px(LINE_H))
                            .border_t_1()
                            .border_color(t.line)
                            .mono_face()
                            .text_size(px(ty::SM))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .flex_1()
        .w_full()
        .min_h(px(LINE_H * 5.));
        match &self.stream {
            Some(h) => list.track_scroll(h).into_any_element(),
            None => list.into_any_element(),
        }
    }
}
