//! The value viewer's body, as what it is: JSON lines that fold, text in the mono face (wrapped or not), bytes as hex
//! rows, and key–value pairs in a well. At most [`MOST_LINES`] are laid out; past it the viewer says how many more
//! there are and Copy has them all.

use doze_desk::page::json::{JsonLine, Tok, visible};
use doze_desk::page::value::{ValueDoc, hex};
use gpui::{
    AnyElement, Div, HighlightStyle, IntoElement, SharedString, StyledText, div, prelude::*, px,
};

use super::blocks::Draw;
use super::work::ValueSlot;
use crate::instance::ui::text;
use crate::theme::{Tokens, alpha, ty};
use crate::widgets::Mono as _;

/// The most lines (or hex rows) laid out at once.
const MOST_LINES: usize = 2000;
/// A line's height in the mono face.
pub(super) const LINE: f32 = 19.;

impl Draw<'_> {
    /// The JSON's lines, each container with a chevron that folds it.
    pub(super) fn json(&self, id: &str, s: &ValueSlot, lines: &[JsonLine]) -> AnyElement {
        let t = self.t;
        let shown = visible(lines, &s.folded);
        let more = shown.len().saturating_sub(MOST_LINES);
        let rows = shown
            .into_iter()
            .take(MOST_LINES)
            .filter_map(|(i, folded)| {
                let l = lines.get(i)?;
                let close = lines.get(l.end).filter(|_| folded);
                let chevron = l.opens().then(|| {
                    let key = id.to_owned();
                    let mut c = div()
                        .id(SharedString::from(format!("fold-{id}-{i}")))
                        .absolute()
                        .left(px(-16. + 16. * l.depth as f32))
                        .top(px(0.))
                        .size(px(LINE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .child(chevron(t, &format!("fold-{id}-{i}"), !folded));
                    if let Some(e) = self.live.clone() {
                        c = c
                            .cursor_pointer()
                            .hover(|s| s.bg(t.sel))
                            .on_click(move |_, _, cx| {
                                e.update(cx, |v, cx| {
                                    if let Some(s) = v.work.values.get_mut(&key)
                                        && !s.folded.remove(&i)
                                    {
                                        s.folded.insert(i);
                                    }
                                    cx.notify();
                                });
                            });
                    }
                    c
                });
                Some(
                    div()
                        .relative()
                        .pl(px(4. + 16. * l.depth as f32))
                        .min_h(px(LINE))
                        .children(chevron)
                        .child(json_line(t, l, folded, close)),
                )
            });
        let mut body = div()
            .flex()
            .flex_col()
            .pl(px(14.))
            .mono_face()
            .text_size(px(ty::SM))
            .line_height(px(LINE))
            .children(rows);
        if more > 0 {
            body = body.child(self.more_lines(more));
        }
        body.into_any_element()
    }

    pub(super) fn more_lines(&self, more: usize) -> Div {
        div()
            .pt(px(6.))
            .font_family(crate::widgets::UI_FAMILY)
            .text_size(px(ty::SM))
            .text_color(self.t.dim)
            .child(text(&format!(
                "{more} more lines: fold what you have read, or Copy to take all of it."
            )))
    }

    pub(super) fn text_body(&self, d: &ValueDoc, nowrap: bool) -> AnyElement {
        let t = self.t;
        // Borrowed, and only the lines shown are laid out; the count was taken once, as the part came.
        let lines = d.text_str().split('\n');
        let more = d.lines.saturating_sub(MOST_LINES);
        let mut body = div()
            .flex()
            .flex_col()
            .min_w_0()
            .mono_face()
            .text_size(px(ty::SM))
            .line_height(px(LINE))
            .text_color(t.bright)
            .children(lines.take(MOST_LINES).map(|l| {
                div()
                    .min_h(px(LINE))
                    .when(nowrap, |d| {
                        d.whitespace_nowrap().overflow_hidden().text_ellipsis()
                    })
                    .child(SharedString::from(l.to_owned()))
            }));
        if more > 0 {
            body = body.child(self.more_lines(more));
        }
        body.into_any_element()
    }

    pub(super) fn hex(&self, d: &ValueDoc) -> AnyElement {
        let t = self.t;
        // Only the rows shown are made: sixteen bytes a row.
        let shown = d.body.len().min(MOST_LINES * 16);
        let rows = hex(d.body.get(..shown).unwrap_or_default());
        let more = d.body.len().div_ceil(16).saturating_sub(MOST_LINES);
        let mut body = div()
            .flex()
            .flex_col()
            .mono_face()
            .text_size(px(ty::SM))
            .line_height(px(LINE))
            .text_color(t.text)
            .children(rows.into_iter().take(MOST_LINES).map(|r| {
                // The offset dim, the bytes as they are, their text bright.
                let (off, rest) = r.split_at(8.min(r.len()));
                let (bytes, ascii) = rest.split_at(51.min(rest.len()));
                div()
                    .flex()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .text_color(t.dim)
                            .child(SharedString::from(off.to_owned())),
                    )
                    .child(SharedString::from(bytes.to_owned()))
                    .child(
                        div()
                            .text_color(t.bright)
                            .child(SharedString::from(ascii.to_owned())),
                    )
            }));
        if more > 0 {
            body = body.child(self.more_lines(more));
        }
        body.into_any_element()
    }
}

/// A container's chevron, turning a quarter as it opens (120 ms; still when motion is reduced).
pub(super) fn chevron(t: &Tokens, id: &str, open: bool) -> AnyElement {
    let (turn, moving) = crate::motion::slide(&format!("{id}/turn"), if open { 0.25 } else { 0. });
    div()
        .flex()
        .child(crate::motion::tick(moving))
        .child(
            gpui::svg()
                .path("icons/chevron.svg")
                .size(px(10.))
                .text_color(t.dim)
                .with_transformation(gpui::Transformation::rotate(gpui::percentage(turn))),
        )
        .into_any_element()
}

/// One JSON line in colour: the key bright, strings, figures and words each their own hue, the marks dim.
fn json_line(t: &Tokens, l: &JsonLine, folded: bool, close: Option<&JsonLine>) -> StyledText {
    let mut s = String::new();
    let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut put = |s: &mut String, piece: &str, color: gpui::Hsla| {
        let at = s.len();
        s.push_str(piece);
        runs.push((
            at..s.len(),
            HighlightStyle {
                color: Some(color),
                ..HighlightStyle::default()
            },
        ));
    };
    if let Some(k) = &l.key {
        put(&mut s, k, t.bright);
        put(&mut s, ": ", t.dim);
    }
    match &l.tok {
        Tok::Open(c) if folded => {
            put(&mut s, &format!("{c} "), t.dim);
            put(&mut s, &l.folded(), alpha(t.dim, 0.9));
            put(
                &mut s,
                &format!(" {}", if *c == '{' { '}' } else { ']' }),
                t.dim,
            );
        }
        Tok::Open(c) | Tok::Close(c) => put(&mut s, &c.to_string(), t.dim),
        Tok::Empty(e) => put(&mut s, e, t.dim),
        Tok::Str(v) => put(&mut s, v, t.cat(0)),
        Tok::Num(v) => put(&mut s, v, t.cat(1)),
        Tok::Lit(v) => put(&mut s, v, t.cat(2)),
    }
    if l.comma || close.is_some_and(|c| c.comma) {
        put(&mut s, ",", t.dim);
    }
    StyledText::new(s).with_highlights(runs)
}

/// Key–value pairs (headers, a hash's fields) in a well: the keys in a column, the values beside them, wrapping.
pub(super) fn pairs(t: &Tokens, pairs: &[(String, String)]) -> AnyElement {
    let widest = pairs
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let key_w = (widest as f32 * 7.4 + 4.).clamp(48., 200.);
    div()
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(t.line)
        .bg(t.well)
        .mono_face()
        .text_size(px(ty::SM))
        .line_height(px(LINE))
        .children(
            pairs
                .iter()
                .enumerate()
                .take(MOST_LINES)
                .map(|(i, (k, v))| {
                    div()
                        .flex()
                        .items_start()
                        .gap(px(12.))
                        .px(px(12.))
                        .py(px(4.))
                        .when(i > 0, |d| d.border_t_1().border_color(t.line))
                        .child(
                            div()
                                .w(px(key_w))
                                .flex_none()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_color(t.dim)
                                .child(SharedString::from(k.clone())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(t.bright)
                                .child(SharedString::from(v.clone())),
                        )
                }),
        )
        .into_any_element()
}
