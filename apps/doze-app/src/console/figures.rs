//! A console's counters and details: each figure a tile (one counted against a limit says `16 of 100`, with a thin
//! meter), and the selected row's detail with what can be done to it.

use doze_desk::page::console::Node;
use doze_display::NodeKind;
use gpui::{AnyElement, Div, IntoElement, div, prelude::*, px};

use super::blocks::{Draw, share};
use crate::instance::ui::{self, heading, text};
use crate::theme::ty;
use crate::widgets::{Mono as _, tracked};

impl Draw<'_> {
    /// The figures on one grid, so the tiles of every row line up and a lone figure is a tile's width, not the page's:
    /// three across where the block shares a row (or the overview's half page), else one a figure, four to six.
    pub(super) fn stats(&self, n: &Node, dir: Option<&NodeKind>) -> AnyElement {
        let across = if self.flat || dir == Some(&NodeKind::Row) {
            3
        } else {
            n.stats.len().clamp(4, 6)
        };
        let tiles = div()
            .grid()
            .grid_cols(u16::try_from(across).unwrap_or(3))
            .gap(px(10.))
            .when(self.flat, |d| d.px(px(14.)).py(px(10.)).gap(px(18.)))
            .children(n.stats.iter().map(|s| self.tile(s)));
        share(div(), n)
            .flex()
            .flex_col()
            // What the figures are ("Channels", "Activity"): on a tab a heading over the tiles, as a table's is.
            .children(if self.flat {
                self.flat_heading(n)
            } else {
                (!n.title.is_empty()).then(|| {
                    div()
                        .px(px(2.))
                        .pb(px(8.))
                        .child(heading(self.t, &n.title, None))
                })
            })
            .child(tiles)
            .into_any_element()
    }

    /// One figure: its label, the figure (and the limit it is counted against, with a meter), or why there is none.
    fn tile(&self, s: &doze_desk::page::console::Stat) -> Div {
        let t = self.t;
        // A figure is a few characters; a module's words ("snapshot when it sleeps") wrap at a smaller size.
        let words = s.value.chars().count() > 12;
        let figure = div()
            .flex()
            .items_baseline()
            .gap(px(6.))
            .min_w_0()
            .when(!words, |d| d.whitespace_nowrap())
            .child(
                div()
                    .text_size(px(
                        if words || (s.ink == doze_desk::page::Ink::Dim && !s.note.is_empty()) {
                            ty::LG_UP
                        } else {
                            ty::FIGURE
                        },
                    ))
                    .when(words, |d| d.whitespace_normal().min_w_0())
                    .font_features(ty::tabular())
                    .font_weight(ty::SEMIBOLD)
                    .text_color(match s.ink {
                        doze_desk::page::Ink::Text => t.bright,
                        i => ui::ink(t, i),
                    })
                    .child(text(&s.value)),
            )
            .when(!s.of.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(ty::SM))
                        .text_color(t.dim)
                        .font_features(ty::tabular())
                        .child(text(&if s.fill.is_some() {
                            format!("of {}", s.of)
                        } else {
                            // A limit in words ("no limit") is said beside the figure, not counted against.
                            format!("· {}", s.of)
                        })),
                )
            });
        let tile = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .min_w_0()
            .child(tracked(&s.label, ty::XS, ty::REGULAR, t.dim, 0.66))
            .child(figure)
            .when(!s.note.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(ty::XS))
                        .text_color(t.dim)
                        .child(text(&s.note)),
                )
            })
            .when_some(s.fill, |d, f| d.child(meter(t, f)));
        if self.flat {
            tile
        } else {
            tile.px(px(14.))
                .py(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(t.line)
                .bg(t.panel)
                .shadow(t.lift())
        }
    }

    pub(super) fn detail(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let mut c = share(self.frame(), n).child(heading(t, &n.title, None));
        match n.detail.split_last() {
            None => c = c.child(div().text_color(t.dim).child("nothing selected")),
            Some(((_, last), rest)) => {
                let facts: Vec<String> = rest
                    .iter()
                    .map(|(k, v)| {
                        if k.is_empty() {
                            v.clone()
                        } else {
                            format!("{} {v}", k.to_lowercase())
                        }
                    })
                    .collect();
                c = c
                    .child(
                        div()
                            .mono_face()
                            .text_size(px(ty::SM))
                            .line_height(px(ty::LH_19))
                            .text_color(t.bright)
                            .child(text(last)),
                    )
                    .child(
                        div()
                            .text_size(px(ty::SM))
                            .text_color(t.dim)
                            .child(text(&facts.join(" · "))),
                    );
            }
        }
        // What can be done to the row it shows.
        if !n.row_actions.is_empty() && !n.detail.is_empty() {
            c = c.child(div().h(px(1.)).bg(t.line).my(px(2.)));
            for (i, a) in n.row_actions.iter().enumerate() {
                c = c.child(self.action_line(a, i, &n.id));
            }
        }
        c.into_any_element()
    }
}

/// A thin meter of how full a figure is against its limit: the accent, amber from 80%, red from 95%.
fn meter(t: &crate::theme::Tokens, f: f32) -> Div {
    let ink = if f >= 0.95 {
        t.bad
    } else if f >= 0.8 {
        t.warn
    } else {
        t.accent
    };
    div()
        .mt(px(2.))
        .h(px(3.))
        .w_full()
        .rounded(px(2.))
        .bg(t.track)
        .child(
            div()
                .h_full()
                .rounded(px(2.))
                .bg(ink)
                .w(gpui::relative(f.max(0.01))),
        )
}
