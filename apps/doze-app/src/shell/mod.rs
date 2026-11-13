//! The window's root: the stack page (title bar, head, vitals, the Map, the keys bar) and the overlays over it. It
//! reads the hub: a batch that touches the shown stack re-reads its view, and the page re-renders only when what it
//! shows changed (the views are `PartialEq`), which is what keeps an idle window at zero redraws.

mod chrome;
mod titlebar;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use doze_desk::{Batch, Hub, StackKey, StackView};
use gpui::{
    AppContext as _, Context, Entity, ExternalPaths, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Window, div, prelude::*, px,
};

use crate::actions::{
    CloseOverlay, CloseWindow, Confirm, CycleMode, MoveDown, MoveUp, NextPalette, OpenPalette,
    Quit, ShowHelp, ViewMap,
};
use crate::map::MapView;
use crate::model::{self, Page};
use crate::overlays::palette::{Palette, PaletteEvent, Pick};
use crate::overlays::{self, Overlay};
use crate::theme::{self, Choice, Mode, Motion, Palette as Colors, tokens};
use crate::widgets::UI_FAMILY;

/// The root view.
pub(crate) struct Root {
    hub: Hub,
    key: Option<StackKey>,
    stack: Option<StackView>,
    page: Page,
    pub(crate) map: Entity<MapView>,
    overlay: Option<Overlay>,
    /// A line at the foot, for a moment (a dropped folder doze cannot open yet, a link it cannot follow yet).
    notice: Option<SharedString>,
    focus: FocusHandle,
}

impl Root {
    /// The page for `first` (else the first running stack).
    pub(crate) fn new(
        hub: Hub,
        first: Option<StackKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let map = cx.new(MapView::new);
        let mut root = Root {
            hub,
            key: first,
            stack: None,
            page: Page::default(),
            map,
            overlay: None,
            notice: None,
            focus: cx.focus_handle(),
        };
        root.refresh(cx);
        let f = root.map.read(cx).focus.clone();
        window.focus(&f, cx);
        root
    }

    /// A batch from the hub: re-read the shown stack if the batch touches it (or the list, while none is shown).
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        let touches = match &self.key {
            Some(k) => !batch.parts(k).is_empty(),
            None => !batch.hub.is_empty() || !batch.stacks.is_empty(),
        };
        if touches {
            self.refresh(cx);
        }
    }

    /// Reads the shown stack; redraws only if the page changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let view = match &self.key {
            Some(k) => self.hub.stack(k),
            None => self.hub.stacks().into_iter().find(|s| s.running),
        };
        if view == self.stack {
            return;
        }
        if let Some(v) = &view {
            self.key = Some(v.key.clone());
        }
        let page = view
            .as_ref()
            .map(|v| model::page(v, SystemTime::now()))
            .unwrap_or_default();
        self.stack = view;
        if page != self.page {
            self.map.update(cx, |m, cx| m.set_page(&page, cx));
            self.page = page;
            cx.notify();
        }
    }

    /// The stack shown.
    pub(crate) fn key(&self) -> Option<&StackKey> {
        self.key.as_ref()
    }

    /// Shows the stack named `name`, and selects the card `show` on its Map.
    pub(crate) fn show(&mut self, name: &str, show: Option<&str>, cx: &mut Context<Self>) -> bool {
        let Some(v) = self.hub.stacks().into_iter().find(|s| s.name == name) else {
            return false;
        };
        self.show_key(v.key, cx);
        if let Some(card) = show {
            self.select(card, cx);
        }
        true
    }

    /// Shows the stack `key`.
    pub(crate) fn show_key(&mut self, key: StackKey, cx: &mut Context<Self>) {
        self.key = Some(key);
        self.stack = None;
        self.refresh(cx);
    }

    /// Selects the card `name` on the Map, centred.
    pub(crate) fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        let found = self.map.update(cx, |m, cx| {
            let id = m.card_named(name);
            if let Some(id) = id {
                m.jump_to(id, cx);
            }
            id.is_some()
        });
        if !found {
            self.say(format!("{name} is not on this stack's map."), cx);
        }
    }

    /// The stack doze runs from `dir`, and the card `show` on it.
    pub(crate) fn open_dir(
        &mut self,
        dir: &std::path::Path,
        show: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let dir = if dir.is_dir() {
            dir.to_path_buf()
        } else {
            dir.parent().map(PathBuf::from).unwrap_or_default()
        };
        match self
            .hub
            .stacks()
            .into_iter()
            .find(|s| std::path::Path::new(&s.dir) == dir)
        {
            Some(s) => {
                self.show_key(s.key, cx);
                if let Some(name) = show {
                    self.select(name, cx);
                }
            }
            None => self.say(
                format!(
                    "No stack runs from {} yet. Opening a folder comes with the Library.",
                    dir.display()
                ),
                cx,
            ),
        }
    }

    /// Says something at the foot for a few seconds.
    pub(crate) fn say(&mut self, words: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(words.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |r, cx| {
                r.notice = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// A folder (or a file in it, a doze.hcl) dropped on the window: its stack, if doze runs one there.
    fn dropped(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        if let Some(p) = paths.paths().first() {
            self.open_dir(&p.clone(), None, cx);
        }
    }

    pub(crate) fn on_palette(
        &mut self,
        _: &OpenPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.overlay, Some(Overlay::Palette(_))) {
            self.close(window, cx);
            return;
        }
        let cards: Vec<(String, String)> = self
            .page
            .rows
            .iter()
            .map(|r| (r.name.clone(), format!("{} · {}", r.look.word(), r.because)))
            .collect();
        let p = cx.new(|cx| Palette::new(cards, window, cx));
        cx.subscribe_in(&p, window, |this, p, ev: &PaletteEvent, window, cx| {
            let PaletteEvent::Chose(pick) = ev;
            let card = match pick {
                Pick::Card(i) => p.read(cx).card(*i).map(str::to_owned),
                Pick::Act(_) => None,
            };
            this.close(window, cx);
            match (pick, card) {
                (Pick::Act(s), _) => window.dispatch_action((s.make)(), cx),
                (Pick::Card(_), Some(name)) => this.map.update(cx, |m, cx| {
                    if let Some(id) = m.card_named(&name) {
                        m.jump_to(id, cx);
                    }
                }),
                _ => {}
            }
        })
        .detach();
        self.overlay = Some(Overlay::Palette(p));
        cx.notify();
    }

    pub(crate) fn on_help(&mut self, _: &ShowHelp, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.overlay, Some(Overlay::Help)) {
            self.close(window, cx);
        } else {
            self.overlay = Some(Overlay::Help);
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Closes the innermost thing open: an overlay, else the Map's find box, else its selection.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.take().is_some() {
            let f = self.map.read(cx).focus.clone();
            window.focus(&f, cx);
            cx.notify();
            return;
        }
        self.map.update(cx, |m, cx| m.escape(window, cx));
    }

    fn set_theme(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        if theme::apply(cx, choice, window.appearance()).is_ok() {
            crate::prefs::save_theme(choice);
            self.map.update(cx, |m, cx| {
                m.cache.clear();
                cx.notify();
            });
            cx.notify();
        }
    }

    fn next_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = *cx.global::<Choice>();
        let i = Colors::ALL
            .iter()
            .position(|p| *p == c.palette)
            .unwrap_or(0);
        let palette = Colors::ALL
            .get((i + 1) % Colors::ALL.len())
            .copied()
            .unwrap_or_default();
        self.set_theme(Choice { palette, ..c }, window, cx);
    }

    fn cycle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = *cx.global::<Choice>();
        let mode = match c.mode {
            Mode::System => Mode::Dark,
            Mode::Dark => Mode::Light,
            Mode::Light => Mode::System,
        };
        self.set_theme(Choice { mode, ..c }, window, cx);
    }

    /// Follows the system's appearance while the mode is System.
    fn follow_system(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = *cx.global::<Choice>();
        if c.mode == Mode::System && c.mode.dark(window.appearance()) != tokens(cx).dark {
            self.set_theme(c, window, cx);
        }
    }

    fn overlay_el(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let t = tokens(cx);
        let motion = Motion::of(cx);
        match &self.overlay {
            Some(Overlay::Palette(p)) => Some(overlays::layer(
                &t,
                p.clone().into_any_element(),
                120.,
                motion,
                "palette",
            )),
            Some(Overlay::Help) => Some(overlays::layer(
                &t,
                overlays::help::sheet(&t, env!("CARGO_PKG_VERSION")).into_any_element(),
                90.,
                motion,
                "help",
            )),
            None => None,
        }
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_system(window, cx);
        let t = tokens(cx);
        let lit = self.map.read(cx).lit.is_some();
        let mut page = div()
            .key_context("Page")
            .flex()
            .flex_col()
            .size_full()
            .child(titlebar::titlebar(&t, &self.page, cx))
            .child(chrome::head(&t, &self.page))
            .child(chrome::strip(&t, &self.page))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .mx(px(28.))
                    .mb(px(14.))
                    .child(self.map.clone()),
            );
        if let Some(n) = &self.notice {
            page = page.child(
                div()
                    .mx(px(28.))
                    .mb(px(10.))
                    .px(px(14.))
                    .py(px(8.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(t.line2)
                    .bg(t.panel)
                    .text_color(t.text)
                    .child(n.clone()),
            );
        }
        page = page.child(chrome::keys_bar(&t, lit));
        div()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context("Root")
            .bg(t.bg)
            .text_color(t.text)
            .text_size(px(13.))
            .line_height(px(20.))
            .font_family(UI_FAMILY)
            .on_action(cx.listener(Self::on_palette))
            .on_action(cx.listener(Self::on_help))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close(window, cx)))
            .on_action(
                cx.listener(|this, _: &NextPalette, window, cx| this.next_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CycleMode, window, cx| this.cycle_mode(window, cx)))
            .on_action(cx.listener(|_, _: &ViewMap, _, _| {}))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.palette_step(1, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.palette_step(-1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.palette_choose(cx)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.dropped(paths, cx)))
            .child(page)
            .children(self.overlay_el(cx))
    }
}

impl Root {
    fn palette_step(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            p.update(cx, |p, cx| p.step(by, cx));
        } else {
            cx.propagate();
        }
    }

    fn palette_choose(&mut self, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            p.update(cx, |p, cx| p.choose(cx));
        } else {
            cx.propagate();
        }
    }
}
