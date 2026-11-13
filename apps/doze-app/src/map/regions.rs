//! A named region's label: a pill on its frame's top edge with its name, how many services it holds, how many are
//! crashing (red) and its memory, at a size that reads at any zoom. A click on it flies to the region; a double click
//! anywhere in the frame does too.

use doze_map::NamedRegion;
use gpui::{AnyElement, Context, MouseButton, SharedString, div, prelude::*, px};

use super::MapView;
use crate::theme::{Tokens, alpha, ty};

/// What a region holds, from its cards' rows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Totals {
    pub(crate) services: u32,
    pub(crate) crashing: u32,
    /// Megabytes resident.
    pub(crate) mb: f64,
}

/// `84 MB` or `1.2 GB` (the board's words) back to megabytes; anything else is none.
pub(crate) fn megabytes(s: &str) -> f64 {
    let mut it = s.split_whitespace();
    let (Some(n), unit) = (it.next().and_then(|n| n.parse::<f64>().ok()), it.next()) else {
        return 0.0;
    };
    match unit {
        Some("GB") => n * 1024.0,
        Some("MB") => n,
        _ => 0.0,
    }
}

/// Megabytes as the board writes a size.
pub(crate) fn size(mb: f64) -> String {
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{} MB", mb.round())
    }
}

impl MapView {
    /// A region's services (a collapsed card counts its members), those in trouble, and their memory; its apps that
    /// fold into the tile (`payments-cron`, nothing calls it) count too, so the frames add up to the header.
    pub(crate) fn totals(&self, r: &NamedRegion) -> Totals {
        let mut t = Totals::default();
        let prefix = format!("{}-", r.name);
        let folded = self
            .layout
            .tile
            .iter()
            .flat_map(|tile| &tile.cards)
            .filter(|&&c| {
                self.layout
                    .card(c)
                    .is_some_and(|c| c.role == doze_map::Role::Lane && c.name.starts_with(&prefix))
            });
        for c in r
            .cards
            .iter()
            .chain(folded)
            .filter_map(|c| self.layout.card(*c))
        {
            let members = self.layout.groups.get(&c.name);
            let names: Vec<&String> = match members {
                Some(m) => m.iter().collect(),
                None => vec![&c.name],
            };
            for n in names {
                t.services += 1;
                if let Some(row) = self.rows.get(n) {
                    t.crashing += u32::from(row.trouble);
                    t.mb += megabytes(&row.mem);
                }
            }
        }
        t
    }
}

/// The labels of the regions in view, and the counts on the links between them.
pub(crate) fn labels(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Vec<AnyElement> {
    let view = m.cam.view();
    let home = m.home_region();
    let mut v = counts(t, m, home);
    v.extend(
        m.layout
            .regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.rect.intersects(&view))
            .map(|(k, r)| label(t, m, k, r, home == Some(k), cx)),
    );
    v
}

/// Each link's count at its middle (`3`, or `2 · 1` both ways), while the links show (zoomed out, lines on demand).
fn counts(t: &Tokens, m: &MapView, home: Option<usize>) -> Vec<AnyElement> {
    // At rest the map shows regions and their health; who talks to whom is read on hover: the counts show only on
    // the links of the region in hand, each from its side (`→ 2` it calls, `← 1` it is called).
    let Some(h) = home.filter(|_| m.quiet() && m.cam.zoom < super::DETAIL_ZOOM) else {
        return Vec::new();
    };
    m.overview
        .links
        .iter()
        .filter(|k| k.a == h || k.b == h)
        .map(|k| {
            let (mine, quiet) = (true, false);
            let (out, inn) = if k.a == h { (k.ab, k.ba) } else { (k.ba, k.ab) };
            let text = match (out, inn) {
                (o, 0) => format!("→ {o}"),
                (0, i) => format!("← {i}"),
                (o, i) => format!("→ {o} · ← {i}"),
            };
            let (x, y) = m.cam.to_screen(k.mid.x, k.mid.y);
            div()
                .absolute()
                .left(px(x - 14.0))
                .top(px(y - 9.0))
                .min_w(px(18.))
                .h(px(18.))
                .px(px(5.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.))
                .border_1()
                .border_color(if mine { t.accent } else { t.line2 })
                .bg(t.panel)
                .text_size(px(ty::COUNT))
                .font_weight(ty::SEMIBOLD)
                .text_color(if mine { t.bright } else { t.dim })
                .when(quiet, |d| d.opacity(0.35))
                .child(SharedString::from(text))
                .into_any_element()
        })
        .collect()
}

fn label(
    t: &Tokens,
    m: &MapView,
    k: usize,
    r: &NamedRegion,
    home: bool,
    cx: &mut Context<MapView>,
) -> AnyElement {
    let (x, y) = m.cam.to_screen(r.rect.x, r.rect.y);
    let totals = m.totals(r);
    // Far out the frames are small: the name alone.
    let full = r.rect.w as f32 * m.cam.zoom > 200.0;
    let mut pill = div()
        .id(("region", k))
        .absolute()
        .left(px(x + 10.0))
        .top(px(y - 11.0))
        .h(px(22.))
        .flex()
        .items_center()
        .gap(px(7.))
        .px(px(9.))
        .rounded(px(11.))
        .border_1()
        .border_color(if home { t.accent } else { t.line2 })
        .bg(t.panel)
        .text_size(px(ty::SM))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|s| s.border_color(t.accent))
        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
            let next = if *over { Some(k) } else { None };
            if this.region_hover != next && (*over || this.region_hover == Some(k)) {
                this.region_hover = next;
                cx.notify();
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| this.open_region(k, cx)))
        .child(
            div()
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(SharedString::from(r.name.clone())),
        );
    if full {
        pill = pill.child(
            div()
                .text_color(t.dim)
                .child(SharedString::from(format!("{} services", totals.services))),
        );
        if totals.crashing > 0 {
            pill = pill.child(
                div()
                    .px(px(6.))
                    .rounded(px(8.))
                    .bg(alpha(t.bad, 0.16))
                    .text_color(t.bad)
                    .child(SharedString::from(format!("{} crashing", totals.crashing))),
            );
        }
        if totals.mb > 0.0 {
            pill = pill.child(
                div()
                    .text_color(t.dim)
                    .child(SharedString::from(size(totals.mb))),
            );
        }
    } else if totals.crashing > 0 {
        pill = pill.child(div().size(px(7.)).rounded_full().bg(t.bad));
    }
    pill.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{megabytes, size};

    #[test]
    fn sizes_read_back() {
        assert!((megabytes("84 MB") - 84.0).abs() < 1e-9);
        assert!((megabytes("1.2 GB") - 1228.8).abs() < 1e-9);
        assert!(megabytes("0").abs() < 1e-9 && megabytes("—").abs() < 1e-9);
        assert_eq!(size(84.0), "84 MB");
        assert_eq!(size(1228.8), "1.2 GB");
    }
}
