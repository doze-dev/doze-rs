//! The toolbar's large-map half: lines on demand (`L`), only trouble (`T`), the focus view's chip (what it shows, its
//! hops, × to leave), and on the right the breadcrumb of the card in hand: Map › its region › it, each a way back out.

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};

use super::chrome::tool;
use super::{Lens, MapView};
use crate::theme::{Tokens, alpha};
use crate::widgets::{kbd_plain, w};

/// The tools a large map adds, and a focus view's chip; nothing on a small map with no focus view.
pub(crate) fn tools(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Vec<AnyElement> {
    let mut v = Vec::new();
    let large = m.base.mode == doze_map::Mode::Layered;
    if large && m.lens.is_none() {
        v.push(
            tool(t, "z-lines", "Lines", m.all_lines)
                .child(kbd_plain("L", 11., t.dim, 500.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.all_lines = !this.all_lines;
                    this.cache.clear();
                    cx.notify();
                }))
                .into_any_element(),
        );
    }
    if large || m.lens.is_some() {
        let on = matches!(m.lens, Some(Lens::Trouble { .. }));
        v.push(
            tool(t, "z-trouble", "Only trouble", on)
                .child(kbd_plain("T", 11., t.dim, 500.))
                .on_click(cx.listener(|this, _, _, cx| this.on_trouble(cx)))
                .into_any_element(),
        );
    }
    if let Some(Lens::Hood { centre, hops }) = &m.lens {
        v.push(chip(t, format!("Around {centre}"), cx));
        v.push(hops_seg(t, *hops, cx));
    }
    v
}

/// The focus view's chip: what it shows, and × to leave it.
fn chip(t: &Tokens, text: String, cx: &mut Context<MapView>) -> AnyElement {
    div()
        .id("z-lens")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(10.))
        .rounded(px(13.))
        .border_1()
        .border_color(alpha(t.accent, 0.6))
        .bg(t.sel)
        .text_size(px(12.))
        .text_color(t.bright)
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| {
            this.leave(cx);
        }))
        .child(SharedString::from(text))
        .child(div().text_color(t.dim).child("×"))
        .into_any_element()
}

/// The neighbourhood's hops: 1 · 2 · 3.
fn hops_seg(t: &Tokens, hops: u8, cx: &mut Context<MapView>) -> AnyElement {
    let mut seg = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .ml(px(4.))
        .text_size(px(12.))
        .child(div().text_color(t.dim).mr(px(2.)).child("hops"));
    for n in 1..=doze_map::MAX_HOPS {
        seg = seg.child(
            tool(t, ("z-hop", usize::from(n)), &n.to_string(), n == hops)
                .on_click(cx.listener(move |this, _, _, cx| this.set_hops(n, cx))),
        );
    }
    seg.into_any_element()
}

/// What the map holds, as the toolbar says it: a focus view's own count, else cards, calls, regions and the zoom.
pub(crate) fn stats(m: &MapView) -> String {
    let l = &m.layout;
    match &m.lens {
        Some(Lens::Trouble {
            troubled,
            between,
            hidden,
        }) => {
            return format!(
                "{troubled} in trouble · {between} between them · {hidden} other cards hidden"
            );
        }
        Some(Lens::Hood { hops, .. }) => {
            return format!(
                "{} cards within {hops} call{} · {:.0}%",
                l.cards.len(),
                if *hops == 1 { "" } else { "s" },
                m.cam.zoom * 100.0
            );
        }
        None => {}
    }
    let calls: u32 = l.edges.iter().map(|e| e.count.max(1)).sum();
    let mut s = format!("{} cards · {calls} calls", l.cards.len());
    if !l.regions.is_empty() {
        s.push_str(&format!(" · {} regions", l.regions.len()));
    }
    s.push_str(&format!(" · {:.0}%", m.cam.zoom * 100.0));
    if super::paint::bundled(m) && l.regions.is_empty() {
        s.push_str(&format!(" · {} bundles", m.overview.bundles.len()));
    }
    s
}

/// Under the pointer in a map by region: what the card's region talks to, from its bundles — `checkout calls payments 3 ·
/// catalog 2 · called by gateway 4` — so the regions' links read at fit without following lines.
pub(crate) fn talks(t: &Tokens, m: &MapView) -> Option<AnyElement> {
    if m.lit.is_some() || m.lens.is_some() {
        return None;
    }
    let k = m.home_region()?;
    let name = |r: usize| {
        m.overview
            .regions
            .get(r)
            .and_then(|r| r.label.clone())
            .unwrap_or_default()
    };
    let list = |pick: &dyn Fn(&doze_map::Bundle) -> Option<usize>| {
        let mut v: Vec<(u32, String)> = m
            .overview
            .bundles
            .iter()
            .filter_map(|b| pick(b).map(|r| (b.count, name(r))))
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        v.iter()
            .take(4)
            .map(|(n, r)| format!("{r} {n}"))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let out = list(&|b| (b.from == k).then_some(b.to));
    let inn = list(&|b| (b.to == k).then_some(b.from));
    let mut parts = Vec::new();
    if !out.is_empty() {
        parts.push(format!("calls {out}"));
    }
    if !inn.is_empty() {
        parts.push(format!("called by {inn}"));
    }
    if parts.is_empty() {
        parts.push("talks to no other region".into());
    }
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .whitespace_nowrap()
            .child(
                div()
                    .font_weight(w(650.))
                    .text_color(t.bright)
                    .child(SharedString::from(name(k))),
            )
            .child(
                div()
                    .text_color(t.text)
                    .child(SharedString::from(parts.join(" · "))),
            )
            .into_any_element(),
    )
}

/// The breadcrumb of the selected card in a map by region: Map › region › card. `None` when there is none to show.
pub(crate) fn crumbs(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Option<AnyElement> {
    if m.lens.is_some() {
        return None;
    }
    let id = m.lit?;
    let card = m.layout.card(id)?;
    let k = m.layout.region_of.get(id.ix()).copied().flatten();
    let region = k.and_then(|k| m.layout.regions.get(k));
    if region.is_none() && !m.layout.folded(id) {
        return None;
    }
    let link = |label: String, id: &'static str| {
        div()
            .id(id)
            .text_color(t.text)
            .cursor_pointer()
            .hover(|s| s.text_color(t.accent))
            .child(SharedString::from(label))
    };
    let sep = || div().text_color(t.dim).child("›");
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(7.))
        .text_size(px(12.))
        .whitespace_nowrap()
        .child(
            link("Map".into(), "crumb-map")
                .on_click(cx.listener(|this, _, _, cx| this.fly_fit(cx))),
        )
        .child(sep());
    match (region, k) {
        (Some(r), Some(k)) => {
            row = row
                .child(
                    link(r.name.clone(), "crumb-region")
                        .on_click(cx.listener(move |this, _, _, cx| this.open_region(k, cx))),
                )
                .child(sep());
        }
        _ => {
            row = row
                .child(div().text_color(t.text).child("with no calls"))
                .child(sep());
        }
    }
    Some(
        row.child(
            div()
                .font_weight(w(650.))
                .text_color(t.bright)
                .child(SharedString::from(card.name.clone())),
        )
        .into_any_element(),
    )
}
