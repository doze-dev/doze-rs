//! The Map's element and its input: the toolbar strip over the body; in the body, the drawing (scene and cards) with
//! the detail card, the minimap and the find box over it. Scroll pans, ⌘/ctrl-scroll and pinch zoom about the
//! pointer, drag pans, a click selects, hovering a card draws its calls. The keys come from the action table.

use doze_map::Rect;
use gpui::{
    AnyElement, BorderStyle, Bounds, Context, FocusHandle, Focusable, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Render, ScrollEvent, Window, canvas,
    div, point, prelude::*, px, quad, size,
};

use super::cache::place;
use super::chrome::{minimap_rect, strip};
use super::detail::{detail, search_box};
use super::paint::scene;
use super::{DETAIL_ZOOM, LABEL_ZOOM, MapView};
use crate::actions::{
    ActualSize, AllLogs, Confirm, FindCard, FitMap, MoveDown, MoveLeft, MoveRight, MoveUp,
    Neighbourhood, OnlyTrouble, ToggleLines, ToggleMinimap, ZoomIn, ZoomOut,
};
use crate::theme::{Tokens, alpha, tokens};

/// A press on the body, until it is let go.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    start: (f32, f32),
    pan: (f32, f32),
    moved: bool,
    minimap: bool,
}

impl MapView {
    /// A large map well past the viewport, so an overview helps (a small map pans a little; a minimap there only
    /// covers cards).
    pub(crate) fn big(&self) -> bool {
        if self.layout.mode != doze_map::Mode::Layered {
            return false;
        }
        let b = self.layout.bounds;
        let (vw, vh) = self.cam.size;
        b.w as f32 * self.cam.zoom > vw * 1.5 || b.h as f32 * self.cam.zoom > vh * 1.5
    }

    /// Zooms about the viewport's middle.
    /// The toolbar's and the keys' zoom eases over the `normal` token; a press during a move adds to where it is going.
    pub(crate) fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let (z, x, y) = self.cam.target();
        let to = (z * factor).clamp(super::camera::MIN_ZOOM, super::camera::MAX_ZOOM);
        let dur = crate::theme::Motion::of(cx).normal;
        if dur.is_zero() {
            // Motion reduced: there at once, about the middle.
            self.cam.zoom_centre(to / self.cam.zoom, self.layout.bounds);
        } else {
            self.cam.fly((to, x, y), self.layout.bounds, dur);
        }
        cx.notify();
    }

    pub(crate) fn on_fit(&mut self, _: &FitMap, _: &mut Window, cx: &mut Context<Self>) {
        self.fly_fit(cx);
    }

    /// `L`: every line, or lines on demand. A small map draws every line already: there `L` is All logs, as
    /// everywhere else.
    pub(crate) fn on_lines(
        &mut self,
        _: &ToggleLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout.mode == doze_map::Mode::Layered {
            self.all_lines = !self.all_lines;
            self.cache.clear();
            cx.notify();
        } else {
            window.dispatch_action(Box::new(AllLogs), cx);
        }
    }

    pub(crate) fn on_find(&mut self, _: &FindCard, window: &mut Window, cx: &mut Context<Self>) {
        self.hit = 0;
        self.open_search(window, cx);
    }

    pub(crate) fn on_minimap(&mut self, _: &ToggleMinimap, _: &mut Window, cx: &mut Context<Self>) {
        self.minimap = !self.minimap;
        cx.notify();
    }

    /// Up and down move through the find box's hits while it is open, else to the nearest card.
    fn arrow(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        if self.search.is_some() && dx == 0.0 {
            let n = self.hits.len().max(1);
            self.hit = if dy > 0.0 {
                (self.hit + 1) % n
            } else {
                (self.hit + n - 1) % n
            };
            cx.notify();
        } else {
            self.step(dx, dy, cx);
        }
    }

    /// ⏎ (Go overview.tsx, the Map's Enter): opens the selected card's page, unless its stack is down; with nothing
    /// selected, selects the first card, so the first ⏎ selects and the second opens.
    fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(&id) = self.hits.get(self.hit) {
            self.jump_to(id, cx);
        } else if let Some(c) = self.lit.and_then(|id| self.layout.card(id)) {
            if self.running {
                cx.emit(super::MapEvent::Open(c.name.clone()));
            }
        } else if self.lit.is_none() {
            self.step(1.0, 0.0, cx);
        }
    }

    /// Viewport pixels for a window position.
    fn local(&self, p: gpui::Point<gpui::Pixels>) -> (f32, f32) {
        let (x, y, _, _) = self.viewport.get();
        (f32::from(p.x) - x, f32::from(p.y) - y)
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (x, y) = self.local(ev.position);
        let mini = minimap_rect(self)
            .is_some_and(|(mx, my, mw, mh)| x >= mx && x <= mx + mw && y >= my && y <= my + mh);
        self.drag = Some(Drag {
            start: (x, y),
            pan: self.cam.pan,
            moved: false,
            minimap: mini,
        });
        if mini {
            self.mini_to(x, y);
            cx.notify();
        }
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (x, y) = self.local(ev.position);
        let Some(d) = self
            .drag
            .as_mut()
            .filter(|_| ev.pressed_button == Some(MouseButton::Left))
        else {
            // Hovering: redraw only when the card under the pointer changes. On a small map the pointer only frames
            // a card (Go `.node:hover`); its lines are drawn by a click.
            let at = self.layout.hit(self.cam.to_map(x, y));
            let framed = at.filter(|_| self.small());
            if framed != self.framed {
                self.framed = framed;
                cx.notify();
            }
            let over = at.filter(|_| !self.small());
            if over != self.hover {
                self.hover = over;
                cx.notify();
            }
            return;
        };
        if (x - d.start.0).abs() + (y - d.start.1).abs() > 3.0 {
            d.moved = true;
            self.cam.flight = None;
        }
        let d = *d;
        if d.minimap {
            self.mini_to(x, y);
        } else if d.moved {
            self.cam.pan = (
                d.pan.0 - (x - d.start.0) / self.cam.zoom,
                d.pan.1 - (y - d.start.1) / self.cam.zoom,
            );
            self.cam.clamp(self.layout.bounds);
        }
        cx.notify();
    }

    fn mouse_up(&mut self, ev: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(d) = self.drag.take() else { return };
        if d.moved || d.minimap {
            return;
        }
        let (x, y) = self.local(ev.position);
        let at = self.cam.to_map(x, y);
        let hit = self.layout.hit(at);
        if ev.click_count >= 2
            && let Some(c) = hit.and_then(|id| self.layout.card(id))
        {
            cx.emit(super::MapEvent::Open(c.name.clone()));
            return;
        }
        // A double click in a region's frame, off its cards, opens the region.
        if ev.click_count >= 2
            && hit.is_none()
            && let Some(k) = self.layout.region_at(at)
        {
            self.open_region(k, cx);
            return;
        }
        // A small map's click selects the card (again, it stays selected; off the cards, nothing changes), as Go's
        // did; on a large one a click on the selection again lets it go.
        if self.small() {
            if hit.is_some() {
                self.select(hit, cx);
            }
            return;
        }
        let next = if hit == self.lit { None } else { hit };
        self.select(next, cx);
    }

    fn scroll(&mut self, ev: &ScrollEvent, cx: &mut Context<Self>) {
        self.cam.flight = None;
        let d = ev.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        if ev.modifiers.platform || ev.modifiers.control {
            let (x, y) = self.local(ev.position);
            self.cam
                .zoom_at((dy * 0.01).exp(), x, y, self.layout.bounds);
        } else {
            self.cam.pan_by(-dx, -dy, self.layout.bounds);
        }
        cx.notify();
    }

    fn pinch(&mut self, ev: &PinchEvent, cx: &mut Context<Self>) {
        self.cam.flight = None;
        let (x, y) = self.local(ev.position);
        self.cam.zoom_at(1.0 + ev.delta, x, y, self.layout.bounds);
        cx.notify();
    }

    /// Centres the view on the minimap point under (x, y).
    fn mini_to(&mut self, x: f32, y: f32) {
        let Some((mx, my, mw, mh)) = minimap_rect(self) else {
            return;
        };
        let b = self.layout.bounds;
        let (fx, fy) = (
            ((x - mx) / mw).clamp(0.0, 1.0),
            ((y - my) / mh).clamp(0.0, 1.0),
        );
        self.cam
            .centre_on(f64::from(fx) * b.w, f64::from(fy) * b.h, b);
    }

    /// The drawing: the scene, and the cards or their names by level.
    fn drawing(&self, t: &Tokens) -> AnyElement {
        let pad = 40.0 / f64::from(self.cam.zoom);
        let v = self.cam.view();
        let vis = self.layout.visible(Rect::new(
            v.x - pad,
            v.y - pad,
            v.w + 2.0 * pad,
            v.h + 2.0 * pad,
        ));
        let on = self.lit_on();
        // A new layout (a focus view opening or closing) fades in.
        let shown = self.fades.shown_in(std::time::Instant::now());
        let mut d = div()
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .size_full()
            .when(shown < 1.0, |d| d.opacity(shown))
            .child(scene(t, self, &vis, self.viewport.clone()));
        if self.cam.zoom >= DETAIL_ZOOM {
            d = d
                .children(super::words::captions(t, self))
                .children(super::cards::cards(t, self, &vis.cards, on.as_deref()));
        } else if self.cam.zoom >= LABEL_ZOOM {
            d = d.children(super::words::labels(t, self, &vis.cards, on.as_deref()));
        }
        d.into_any_element()
    }

    /// The overlays on the drawing that belong to a large map: the regions' labels, the tile or its list.
    fn large(&self, t: &Tokens, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut v = super::regions::labels(t, self, cx);
        v.extend(super::tools::talks(t, self));
        v.extend(super::tile::tile(t, self, cx));
        v.extend(super::tile::list(t, self, cx));
        v
    }
}

/// The whole drawing small, every card a speck, the viewport outlined; press or drag on it to move there.
fn minimap(t: &Tokens, m: &MapView) -> Option<AnyElement> {
    let (x, y, w, h) = minimap_rect(m)?;
    let (t, l, cache, view, focus) = (
        *t,
        m.layout.clone(),
        m.cache.clone(),
        m.cam.view(),
        m.focus_card(),
    );
    let drawn = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let s = w / l.bounds.w as f32;
            let o = bounds.origin;
            let colors = [alpha(t.text, 0.4), alpha(t.accent, 0.5)];
            cache.mini(&l, s, &mut |p, c| {
                window.paint_path(place(p, 1.0, o), colors.get(c).copied().unwrap_or(t.dim))
            });
            if let Some(c) = focus.and_then(|id| l.card(id)) {
                let r = c.rect;
                let b = Bounds {
                    origin: point(o.x + px(r.x as f32 * s), o.y + px(r.y as f32 * s)),
                    size: size(px((r.w as f32 * s).max(3.)), px((r.h as f32 * s).max(3.))),
                };
                window.paint_quad(gpui::fill(b, t.accent));
            }
            let vb = Bounds {
                origin: point(o.x + px(view.x as f32 * s), o.y + px(view.y as f32 * s)),
                size: size(px(view.w as f32 * s), px(view.h as f32 * s)),
            };
            window.paint_quad(quad(
                vb,
                px(2.),
                alpha(t.accent, 0.08),
                px(1.),
                t.accent,
                BorderStyle::Solid,
            ));
        },
    )
    .size_full();
    Some(
        div()
            .absolute()
            .left(px(x - 4.))
            .top(px(y - 4.))
            .w(px(w + 8.))
            .h(px(h + 8.))
            .p(px(4.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.line2)
            .bg(alpha(t.panel, 0.82))
            .child(drawn)
            .into_any_element(),
    )
}

impl Focusable for MapView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MapView {
    /// The drawing in the Map's key context: its keys bind while it has focus.
    fn keyed(&self, t: &Tokens, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .key_context("Map")
            .track_focus(&self.focus)
            .absolute()
            .size_full()
            .on_action(cx.listener(Self::on_fit))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_minimap))
            .on_action(cx.listener(Self::on_lines))
            .on_action(cx.listener(|this, _: &Neighbourhood, _, cx| this.on_hood(cx)))
            .on_action(cx.listener(|this, _: &OnlyTrouble, _, cx| this.on_trouble(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1.25, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(0.8, cx)))
            .on_action(cx.listener(|this, _: &ActualSize, _, cx| {
                let z = this.cam.zoom;
                this.zoom_by(1.0 / z, cx);
            }))
            .child(self.drawing(t))
    }
}

impl Render for MapView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let (_, _, w, h) = self.viewport.get();
        // A small map is fitted again when its box changes width, as Go's re-measures (ResizeObserver).
        let narrower = (self.cam.size.0 - w).abs() > 0.5 && self.small();
        self.cam.size = (w, h);
        if (!self.cam.fitted || narrower) && w > 1.0 && !self.layout.cards.is_empty() {
            self.refit();
        }
        let now = std::time::Instant::now();
        let flying = self.cam.tick(now);
        let motion = crate::theme::Motion::of(cx);
        let times = super::fade::times(motion);
        let on = self.lit_on().map(std::rc::Rc::new);
        self.fades.note(self.lit, on, self.hover, times, now);
        if flying || self.fades.running(now) {
            window.request_animation_frame();
        }
        let mut body = div()
            .id("map-body")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev, window, cx| this.mouse_down(ev, window, cx)),
            )
            .on_mouse_move(cx.listener(|this, ev, _, cx| this.mouse_move(ev, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev, _, cx| this.mouse_up(ev, cx)),
            )
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if !*over && (this.hover.take().is_some() | this.framed.take().is_some()) {
                    cx.notify();
                }
            }))
            .on_scroll(cx.listener(|this, ev, _, cx| this.scroll(ev, cx)))
            .on_pinch(cx.listener(|this, ev, _, cx| this.pinch(ev, cx)))
            .child(self.keyed(&t, cx))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.arrow(0.0, -1.0, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.arrow(0.0, 1.0, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.arrow(-1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.arrow(1.0, 0.0, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.confirm(cx)));
        self.note_keys(window, cx);
        let large = self.large(&t, cx);
        let window_w = f32::from(window.viewport_size().width);
        body = body
            .children(large)
            .children(super::chrome::legend(&t, self, window_w))
            .children(minimap(&t, self))
            .children(detail(&t, self, cx));
        if let Some(s) = self.search.clone() {
            body = body.child(search_box(&t, self, &s, cx));
        }
        div()
            .flex()
            .flex_col()
            .size_full()
            .rounded(px(12.))
            .border_1()
            .border_color(t.line)
            .bg(t.well)
            .overflow_hidden()
            .child(strip(&t, self, cx))
            .child(body)
    }
}
