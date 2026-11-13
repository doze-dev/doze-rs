//! The cards with no calls, folded (past sixty cards): one tile at the drawing's bottom left, "23 with no calls", and
//! the list it opens over the drawing's lower part, searchable and filtered (no callers, modules, services, in
//! trouble), trouble first. Picking a row selects its card: the detail card says what it is, and a shared one lights
//! the cards that use it.

use doze_map::{CardId, Kind, Role};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, MouseButton, SharedString, Window, div,
    prelude::*, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use super::MapView;
use super::cards::state_dot;
use super::chrome::enter;
use crate::model::state_index;
use crate::theme::{Motion, Tokens, alpha, ty};

/// The tile's height; the camera keeps this much clear at the bottom when it fits.
pub(crate) const TILE_H: f32 = 34.;

/// Which of the folded the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    Lane,
    Modules,
    Services,
    Trouble,
}

/// The list, open.
pub(crate) struct List {
    pub(crate) input: Entity<InputState>,
    pub(crate) filter: Filter,
}

impl MapView {
    /// Opens the tile's list, its search box focused.
    pub(crate) fn open_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name"));
        cx.subscribe(&input, |this: &mut MapView, _, ev: &InputEvent, cx| {
            if let InputEvent::PressEnter { .. } = ev
                && let Some(&id) = this.listed(cx).first()
            {
                this.pick(id, cx);
            }
            cx.notify();
        })
        .detach();
        input.update(cx, |i, cx| i.focus(window, cx));
        self.list = Some(List {
            input,
            filter: Filter::All,
        });
        cx.notify();
    }

    /// Selects a folded card from the list, and closes it.
    fn pick(&mut self, id: CardId, cx: &mut Context<Self>) {
        self.list = None;
        self.lit = Some(id);
        cx.notify();
    }

    fn passes(&self, id: CardId, f: Filter) -> bool {
        let Some(c) = self.layout.card(id) else {
            return false;
        };
        match f {
            Filter::All => true,
            Filter::Lane => c.role == Role::Lane,
            Filter::Modules => c.role == Role::Shared && c.kind == Kind::Module,
            Filter::Services => c.role == Role::Shared && c.kind == Kind::Service,
            Filter::Trouble => self.rows.get(&c.name).is_some_and(|r| r.trouble),
        }
    }

    /// The rows the list shows: its filter and its search, trouble first, then awake or dozing, then by name.
    pub(crate) fn listed(&self, cx: &gpui::App) -> Vec<CardId> {
        let (Some(tile), Some(list)) = (self.layout.tile.as_ref(), self.list.as_ref()) else {
            return Vec::new();
        };
        let q = list.input.read(cx).value().trim().to_lowercase();
        let mut v: Vec<(u8, &str, CardId)> = tile
            .cards
            .iter()
            .filter(|&&id| self.passes(id, list.filter))
            .filter_map(|&id| {
                let c = self.layout.card(id)?;
                if !q.is_empty() && !c.name.to_lowercase().contains(&q) {
                    return None;
                }
                let order = match state_index(self.rows.get(&c.name)) {
                    4 => 0,
                    0 | 1 => 1,
                    _ => 2,
                };
                Some((order, c.name.as_str(), id))
            })
            .collect();
        v.sort_unstable();
        v.into_iter().map(|(_, _, id)| id).collect()
    }
}

/// The tile at the drawing's bottom left, when the map folds and nothing covers it.
pub(crate) fn tile(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Option<AnyElement> {
    let tile = m.layout.tile.as_ref().filter(|_| m.list.is_none())?;
    let trouble = tile
        .cards
        .iter()
        .filter(|id| m.passes(**id, Filter::Trouble))
        .count();
    // Up close the tile is its count alone, so it covers as little of the cards as it can.
    let close = m.cam.zoom >= super::DETAIL_ZOOM;
    let mut row = div()
        .id("map-tile")
        .occlude()
        .absolute()
        .left(px(12.))
        .bottom(px(12.))
        .h(px(TILE_H))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(12.))
        .rounded(px(9.))
        .border_1()
        .border_dashed()
        .border_color(t.line2)
        .bg(alpha(t.panel, 0.94))
        .text_size(px(ty::SM))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|s| s.border_color(t.accent))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, window, cx| this.open_list(window, cx)))
        .child(
            div()
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(SharedString::from(format!(
                    "{} with no calls",
                    tile.cards.len()
                ))),
        )
        .when(!close, |d| {
            d.child(div().text_color(t.dim).child(SharedString::from(format!(
                "{} apps nothing calls · {} shared",
                tile.lane, tile.shared
            ))))
        });
    if trouble > 0 && !close {
        row = row.child(
            div()
                .text_color(t.bad)
                .child(SharedString::from(format!("{trouble} in trouble"))),
        );
    }
    Some(
        row.child(div().ml(px(6.)).text_color(t.accent).child("open ▸"))
            .into_any_element(),
    )
}

fn chip(
    t: &Tokens,
    label: String,
    on: bool,
    f: Filter,
    cx: &mut Context<MapView>,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("tile-f-{f:?}")))
        .h(px(24.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(12.))
        .border_1()
        .border_color(if on { t.accent } else { t.line2 })
        .bg(if on { t.sel } else { t.panel })
        .text_color(if on { t.bright } else { t.text })
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(l) = this.list.as_mut() {
                l.filter = f;
            }
            cx.notify();
        }))
        .child(SharedString::from(label))
}

/// The filters, each with its count.
fn chips(t: &Tokens, m: &MapView, on: Filter, cx: &mut Context<MapView>) -> gpui::Div {
    let cards = m
        .layout
        .tile
        .as_ref()
        .map_or(&[][..], |t| t.cards.as_slice());
    let count = |f: Filter| cards.iter().filter(|id| m.passes(**id, f)).count();
    let mut row = div().flex().items_center().gap(px(6.));
    for (f, word) in [
        (Filter::All, "All"),
        (Filter::Lane, "Nothing calls"),
        (Filter::Modules, "Modules"),
        (Filter::Services, "Services"),
        (Filter::Trouble, "In trouble"),
    ] {
        row = row.child(chip(t, format!("{word} {}", count(f)), on == f, f, cx));
    }
    row
}

/// The list, over the drawing's lower part.
pub(crate) fn list(t: &Tokens, m: &MapView, cx: &mut Context<MapView>) -> Option<AnyElement> {
    let (tile, l) = (m.layout.tile.as_ref()?, m.list.as_ref()?);
    let chips = chips(t, m, l.filter, cx);
    let shown = m.listed(cx);
    let mut grid = div().flex().flex_wrap().gap(px(6.)).pt(px(10.));
    for &id in &shown {
        grid = grid.child(row(t, m, id, cx));
    }
    if shown.is_empty() {
        grid = grid.child(div().text_color(t.dim).child("nothing matches"));
    }
    // Placed from the top left: the entrance wraps the panel, so it cannot hang from the bottom.
    let (vw, vh) = m.cam.size;
    let h = (vh * 0.62).max(220.).min(vh - 24.);
    let panel = div()
        .id("map-tile-list")
        .occlude()
        .absolute()
        .left(px(12.))
        .top(px(vh - 12. - h))
        .w(px(vw - 24.))
        .h(px(h))
        .flex()
        .flex_col()
        .p(px(14.))
        .rounded(px(12.))
        .border_1()
        .border_color(t.line2)
        .bg(t.panel)
        .text_size(px(ty::SM_UP))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .child(head(t, tile.cards.len(), tile.lane, tile.shared))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .pt(px(10.))
                .child(div().w(px(280.)).child(Input::new(&l.input)))
                .child(chips)
                .child(
                    div()
                        .ml_auto()
                        .text_color(t.dim)
                        .child("trouble first, then awake or dozing, then by name"),
                ),
        )
        .child(
            div()
                .id("map-tile-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(grid),
        );
    Some(enter(panel, "map-tile-in", Motion::of(cx), 0.0, 1.0))
}

fn head(t: &Tokens, n: usize, lane: u32, shared: u32) -> impl IntoElement {
    div()
        .flex()
        .items_baseline()
        .gap(px(10.))
        .child(
            div()
                .text_size(px(ty::MD))
                .font_weight(ty::HEADING)
                .text_color(t.bright)
                .child(SharedString::from(format!("{n} with no calls"))),
        )
        .child(div().text_color(t.dim).child(SharedString::from(format!(
            "{lane} apps nothing calls · {shared} shared modules and services (used, not called: the chips on each card say who uses what)"
        ))))
        .child(div().ml_auto().text_color(t.dim).child("esc"))
}

fn row(t: &Tokens, m: &MapView, id: CardId, cx: &mut Context<MapView>) -> AnyElement {
    let Some(c) = m.layout.card(id) else {
        return div().into_any_element();
    };
    let r = m.rows.get(&c.name);
    let trouble = r.is_some_and(|r| r.trouble);
    let note = if c.role == Role::Shared {
        match c.uses.len() {
            0 => "used by none".to_string(),
            n => format!("used by {n}"),
        }
    } else {
        r.map(|r| r.mem.clone())
            .filter(|m| !m.is_empty() && m != "0")
            .unwrap_or_else(|| r.map(|r| r.look.word().to_string()).unwrap_or_default())
    };
    div()
        .id(("tile-row", id.ix()))
        .w(px(232.))
        .h(px(30.))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .rounded(px(7.))
        .border_1()
        .border_color(if trouble { alpha(t.bad, 0.5) } else { t.line })
        .bg(t.well)
        .cursor_pointer()
        .hover(|s| s.border_color(t.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.pick(id, cx)))
        .child(state_dot(t, px(7.), r))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(t.bright)
                .child(SharedString::from(c.name.clone())),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(ty::XS))
                .text_color(if trouble { t.bad } else { t.dim })
                .child(SharedString::from(if trouble {
                    "in trouble".to_string()
                } else {
                    note
                })),
        )
        .into_any_element()
}
