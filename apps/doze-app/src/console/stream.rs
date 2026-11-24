//! A console's streams: rows as they arrive, the newest kept in view (see `ConsoleTabView::refresh`); one with no
//! lines yet is a single line saying so, not a block taking the room left.

use doze_desk::page::console::Node;
use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px, uniform_list};

use super::blocks::{Draw, share};
use super::parts::{LINE_H, MONO_CH, widths};

/// A stream card's height besides its lines: its title, the column titles and the border.
const CHROME: f32 = 34. + 30. + 2.;
/// The fewest lines a busy stream shows, however short the page.
const FEWEST: usize = 5;
use crate::theme::ty;
use crate::widgets::Mono as _;

impl Draw<'_> {
    pub(super) fn stream(&self, n: &Node) -> AnyElement {
        let t = *self.t;
        let widths = widths(n, MONO_CH);
        let empty = n.rows.is_empty();
        let body: AnyElement = if empty {
            div()
                .px(px(16.))
                .pb(px(14.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(if self.asleep {
                    "Asleep: its lines show while it is awake."
                } else {
                    "Waiting for the first line."
                })
                .into_any_element()
        } else {
            let shown = self.lines_shown(n);
            self.keep_newest_in_view(n, shown);
            self.lines(n, &widths, shown)
        };
        let card = if self.flat {
            div().flex().flex_col().overflow_hidden()
        } else {
            div()
                .flex()
                .flex_col()
                .rounded(px(10.))
                .border_1()
                .border_color(t.line)
                .bg(t.panel)
                .shadow(t.lift())
                .overflow_hidden()
        }
        .flex_none()
        .min_w_0()
        .children(self.title_bar(n))
        .when(!empty, |d| d.child(self.header(n, &widths, false)))
        .child(body);
        // The card is as tall as its lines, up to the room its place in the layout has (measured as it is laid out,
        // `Draw::rooms`): a few lines are a short card, not a tall empty one, and a busy stream shows whole lines only,
        // so the newest is at the bottom and the oldest shown is not cut under the header.
        let mut slot = share(div(), n).flex().flex_col().relative();
        if let (false, Some(rooms)) = (empty, self.rooms.clone()) {
            let (id, view) = (n.id.clone(), self.live.clone());
            // No taller than its lines need: the room below a short stream is the page's, not a gap before the next block.
            #[allow(clippy::cast_precision_loss)] // WHY: a count of lines on a page
            let all = CHROME + LINE_H * n.rows.len() as f32;
            slot = slot
                .min_h(px(all.min(CHROME + LINE_H * FEWEST as f32)))
                .max_h(px(all))
                .child(
                    gpui::canvas(
                        move |bounds, window, cx| {
                            let h = bounds.size.height / px(1.);
                            let mut r = rooms.borrow_mut();
                            if r.get(&id).is_none_or(|old| (old - h).abs() > 0.5) {
                                r.insert(id, h);
                                // Drawn again with the lines that fit (only when the room changed: an idle window stays still).
                                if let Some(v) = &view {
                                    cx.notify(v.entity_id());
                                }
                                window.refresh();
                            }
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .size_full(),
                );
        }
        slot.child(card).into_any_element()
    }

    /// A list drawn with a different number of lines keeps its scroll offset, which then stops a line short of the
    /// newest (or past it): when the lines shown change, the newest goes back to the bottom.
    fn keep_newest_in_view(&self, n: &Node, shown: usize) {
        let (Some(rooms), Some(follow)) = (&self.rooms, &self.stream) else {
            return;
        };
        let key = format!("{}#shown", n.id);
        let mut r = rooms.borrow_mut();
        #[allow(clippy::cast_precision_loss)] // WHY: a count of lines on a page
        let now = shown as f32;
        if r.get(&key) != Some(&now) {
            r.insert(key, now);
            follow.scroll_to_bottom();
        }
    }

    /// How many lines a stream shows: all of them, up to as many whole lines as its room holds.
    fn lines_shown(&self, n: &Node) -> usize {
        let room = self
            .rooms
            .as_ref()
            .and_then(|r| r.borrow().get(&n.id).copied())
            .unwrap_or(CHROME + LINE_H * 8.);
        let fit = ((room - CHROME) / LINE_H).floor().max(FEWEST as f32) as usize;
        n.rows.len().min(fit)
    }

    /// A stream's lines, the newest kept in view.
    fn lines(&self, n: &Node, widths: &[Option<f32>], shown: usize) -> AnyElement {
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
                    asleep: false,
                    rooms: None,
                };
                range
                    .filter_map(|i| rows.get(i))
                    .map(|r| {
                        d.cells(&node, &widths, d.row_cells(r), None)
                            .h(px(LINE_H))
                            .border_t_1()
                            .border_color(t.line)
                            .mono_face()
                            .text_size(px(ty::SM))
                    })
                    .collect::<Vec<_>>()
            },
        )
        .w_full()
        .h(px(LINE_H * shown as f32));
        match &self.stream {
            Some(h) => list.track_scroll(h).into_any_element(),
            None => list.into_any_element(),
        }
    }
}
