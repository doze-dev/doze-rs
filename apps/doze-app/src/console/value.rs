//! The value viewer (module protocol minor 2): one value in full, beside the table or tree it follows. JSON pretty
//! and folded (a click on a container's chevron folds or opens it), text wrapped in the mono face, bytes as hex rows,
//! key–value pairs (headers, a hash's fields) above the body. The first part comes in one answer with the whole
//! size beside it; "Load the rest" asks for more. An instance that sleeps is woken only by the press that opens the
//! value; one that is off, failed or starting says so in the daemon's words and is not woken.

use doze_desk::page::console::Node;
use doze_desk::page::value::{Refusal, Shown, ValueDoc};
use doze_desk::page::viewer::Viewer;
use gpui::{AnyElement, Div, Entity, IntoElement, SharedString, div, prelude::*, px};

use super::ConsoleTabView;
use super::blocks::{Draw, share};
pub(super) use super::value_body::chevron;
use super::value_body::pairs;
use super::work::ValueSlot;
use crate::instance::ui::{heading, text};
use crate::kit::{Btn, btn};
use crate::theme::ty;
use crate::widgets::Mono as _;

/// What the viewer does when pressed.
fn on(
    live: &Option<Entity<ConsoleTabView>>,
    f: impl Fn(&mut ConsoleTabView, &mut gpui::Context<ConsoleTabView>) + 'static,
) -> Option<crate::kit::Click> {
    let e = live.clone()?;
    Some(Box::new(move |_, _, cx| e.update(cx, |v, cx| f(v, cx))))
}

impl Draw<'_> {
    /// The viewer `n`; `own` names the slot when a tree shows its leaves itself.
    pub(super) fn value(&self, n: &Node, own: Option<&str>) -> AnyElement {
        let t = self.t;
        let id = own.map_or_else(|| n.id.clone(), str::to_owned);
        let slot = self.work.and_then(|w| w.values.get(&id));
        let want = slot.and_then(|s| s.viewer.want.as_ref());
        let title = if own.is_some() {
            "Value"
        } else {
            n.title.as_str()
        };
        // In a narrow card the tools go under the title rather than past the card's edge.
        let mut head = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(px(8.))
            .gap_y(px(6.))
            .min_w_0()
            .child(heading(t, title, None))
            .children(want.map(|w| {
                div()
                    .flex_1()
                    .min_w(px(48.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .mono_face()
                    .text_size(px(ty::SM))
                    .text_color(t.bright)
                    .child(text(&w.label))
            }))
            .when(want.is_none(), |d| d.child(div().flex_1()));
        if let Some(d) = slot.and_then(|s| s.viewer.doc.as_ref()) {
            head = head.child(self.tools(&id, slot, d));
        }
        let body = match (want, slot) {
            (None, _) | (_, None) => self.quiet(if n.follows_tree || own.is_some() {
                "Open a leaf to see its value here."
            } else if self.work.is_none() {
                "Values open on the console's own tab."
            } else {
                "Pick a row to see its value."
            }),
            (Some(_), Some(s)) => match (&s.viewer.doc, &s.viewer.refused) {
                (_, Some(r)) => self.refusal(&id, r),
                (Some(d), None) => self.doc(&id, s, d),
                (None, None) => self.reading(&id),
            },
        };
        share(self.frame(), n)
            .min_h(px(160.))
            .child(head)
            .child(
                div()
                    .id(SharedString::from(format!("value-body-{id}")))
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    /// Words where a value would be.
    fn quiet(&self, words: &str) -> AnyElement {
        div()
            .py(px(6.))
            .text_size(px(ty::SM))
            .text_color(self.t.dim)
            .child(text(words))
            .into_any_element()
    }

    fn reading(&self, id: &str) -> AnyElement {
        let t = self.t;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(6.))
            .text_size(px(ty::SM))
            .text_color(t.dim)
            .child(crate::widgets::spinner(
                SharedString::from(format!("value-spin-{id}")),
                12.,
                t.dim,
            ))
            .child("Reading the value…")
            .into_any_element()
    }

    /// Why it could not be read, in the daemon's words, and the one thing to press when there is one: open (waking
    /// it) for an instance that sleeps, try again for one starting or a module that failed to answer.
    fn refusal(&self, id: &str, r: &Refusal) -> AnyElement {
        let t = self.t;
        let (press, kind) = if r.asleep() {
            (Some("Open and wake it"), Btn::Primary)
        } else if r.starting() || r.state().is_none() {
            (Some("Try again"), Btn::Plain)
        } else {
            (None, Btn::Plain)
        };
        let wake = r.asleep();
        let key = id.to_owned();
        let click = on(&self.live, move |v, cx| {
            v.value_ask(&key, |w| if wake { w.open_waking() } else { w.retry() }, cx);
        });
        let tone = match r.state() {
            Some(doze_control::fault::FAILED) => t.bad,
            Some(_) => t.text,
            None => t.bad,
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(8.))
            .bg(t.well)
            .border_1()
            .border_color(t.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(r.asleep(), |d| {
                        d.child(crate::widgets::icon("icons/moon.svg", 13., t.dim))
                    })
                    .child(div().text_color(tone).child(text(&r.message))),
            )
            .children(press.filter(|_| self.live.is_some()).map(|label| {
                btn(
                    t,
                    SharedString::from(format!("value-press-{id}")),
                    label,
                    "",
                    kind,
                    click,
                )
            }))
            .into_any_element()
    }

    /// The header's tools once there is a value: what it is and how big, wrap (text), copy.
    fn tools(&self, id: &str, slot: Option<&ValueSlot>, d: &ValueDoc) -> Div {
        let t = self.t;
        let mut row = div().flex().flex_none().items_center().gap(px(6.)).child(
            div()
                .px(px(8.))
                .h(px(22.))
                .flex()
                .items_center()
                .rounded(px(6.))
                .bg(t.well)
                .text_size(px(ty::XS))
                .font_features(ty::tabular())
                .text_color(t.dim)
                .child(text(&d.summary())),
        );
        if self.live.is_none() {
            return row;
        }
        if d.shown() == Shown::Text && !d.body.is_empty() {
            let nowrap = slot.is_some_and(|s| s.nowrap);
            let key = id.to_owned();
            row = row.child(btn(
                t,
                SharedString::from(format!("value-wrap-{id}")),
                if nowrap { "Wrap" } else { "No wrap" },
                "",
                Btn::Quiet,
                on(&self.live, move |v, cx| {
                    if let Some(s) = v.work.values.get_mut(&key) {
                        s.nowrap = !s.nowrap;
                    }
                    cx.notify();
                }),
            ));
        }
        let copy_id = format!("value-copy-{id}");
        let copied = slot
            .and_then(|s| s.copied)
            .is_some_and(|at| at.elapsed() < crate::kit::COPIED_FOR);

        let (cid, key) = (copy_id.clone(), id.to_owned());
        row.child(btn(
            t,
            SharedString::from(copy_id),
            if copied { "Copied" } else { "Copy" },
            "",
            Btn::Plain,
            on(&self.live, move |v, cx| {
                // Made when pressed, not each time the viewer is drawn.
                let all = v
                    .work
                    .values
                    .get(&key)
                    .and_then(|s| s.viewer.doc.as_ref())
                    .map(|d| d.copy_text())
                    .unwrap_or_default();
                crate::outward::copy(cx, &all);
                if let Some(s) = v.work.values.get_mut(&key) {
                    s.copied = Some(std::time::Instant::now());
                }
                crate::kit::flip_copied(cid.clone(), cx);
            }),
        ))
    }

    /// The value: its pairs, its body as what it is, and how much more there is.
    fn doc(&self, id: &str, s: &ValueSlot, d: &ValueDoc) -> AnyElement {
        let t = self.t;
        let mut out = div().flex().flex_col().gap(px(10.)).min_w_0();
        if !d.pairs.is_empty() {
            out = out.child(pairs(t, &d.pairs));
        }
        if !d.body.is_empty() {
            out = out.children(
                d.json_note()
                    .map(|n| div().text_size(px(ty::XS)).text_color(t.dim).child(text(n))),
            );
            out = out.child(match (d.shown(), &d.json) {
                (Shown::Json, Some(lines)) => self.json(id, s, lines),
                (Shown::Bytes, _) => self.hex(d),
                _ => self.text_body(d, s.nowrap),
            });
        } else if d.pairs.is_empty() {
            out = out.child(self.quiet("Empty."));
        }
        if d.more {
            let key = id.to_owned();
            out = out.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pt(px(2.))
                    .child(
                        div()
                            .text_size(px(ty::SM))
                            .text_color(t.dim)
                            .child(text(&format!("Showing {}.", d.summary()))),
                    )
                    .when(s.viewer.loading, |r| {
                        r.child(crate::widgets::spinner(
                            SharedString::from(format!("value-more-{id}")),
                            12.,
                            t.dim,
                        ))
                    })
                    .when(!s.viewer.loading && self.live.is_some(), |r| {
                        r.child(btn(
                            t,
                            SharedString::from(format!("value-rest-{id}")),
                            "Load the rest",
                            "",
                            Btn::Plain,
                            on(&self.live, move |v, cx| v.value_ask(&key, Viewer::rest, cx)),
                        ))
                    }),
            );
        }
        crate::motion::settle("value-doc", out, 160)
    }
}
