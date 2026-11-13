//! The root's overlays (⌘K and the help sheet), the theme and the density.

use gpui::{AppContext as _, Context, IntoElement, Window};

use super::{Root, Screen};
use crate::actions::{OpenPalette, ShowHelp};
use crate::overlays::palette::{Palette, PaletteEvent};
use crate::overlays::{self, Overlay};
use crate::overview::View;
use crate::theme::{self, Choice, Density, Mode, Motion, Palette as Colors, tokens};

impl Root {
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
        let current = self
            .key()
            .map(|k| k.dir().to_string_lossy().into_owned())
            .unwrap_or_default();
        let inst = match &self.screen {
            Screen::Instance(_, n) => n.clone(),
            _ => String::new(),
        };
        let hub = self.hub.clone();
        let p = cx.new(|cx| Palette::new(hub, current, inst, window, cx));
        cx.subscribe_in(&p, window, |this, _, ev: &PaletteEvent, window, cx| {
            let PaletteEvent::Do(d) = ev;
            let d = d.clone();
            this.shut(window, cx);
            this.run_doing(&d, window, cx);
        })
        .detach();
        self.close_bell();
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

    /// Closes the innermost thing open: an overlay, else what the page inside holds or the page itself, else the Map's
    /// find box or its selection, else the stack's page itself (back to Home).
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ⌘K's esc clears its box, then steps out, and only then closes.
        if let Some(Overlay::Palette(p)) = &self.overlay
            && p.update(cx, |p, cx| p.escape(window, cx))
        {
            return;
        }
        let on_map =
            matches!(self.screen, Screen::Stack(_)) && self.overview.read(cx).view == View::Map;
        if self.shut(window, cx) {
            return;
        }
        if self.acts.read(cx).recent {
            self.acts.update(cx, |a, cx| a.toggle_recent(cx));
            return;
        }
        if self.escape_inside(window, cx) {
            return;
        }
        let map_held = on_map && { self.map.read(cx).holds() };
        if map_held {
            self.map.update(cx, |m, cx| m.escape(window, cx));
        } else if matches!(self.screen, Screen::Stack(_) | Screen::Empty(_)) {
            // Nothing open, nothing held: esc goes home, as the Go page's did.
            self.go(Screen::Library, window, cx);
        }
    }

    /// Closes the overlay open, if one is, and gives the keys back to the page; false when none was open.
    pub(crate) fn shut(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.overlay.take().is_none() {
            return false;
        }
        if self.focus_inside(window, cx) {
            cx.notify();
            return true;
        }
        let on_map =
            matches!(self.screen, Screen::Stack(_)) && self.overview.read(cx).view == View::Map;
        let f = match &self.screen {
            Screen::Stack(_) if on_map => self.map.read(cx).focus.clone(),
            Screen::Stack(_) => self.overview.read(cx).focus.clone(),
            Screen::Library | Screen::Empty(_) | Screen::Instance(..) | Screen::Logs(_) => {
                self.library.read(cx).focus.clone()
            }
        };
        window.focus(&f, cx);
        cx.notify();
        true
    }

    pub(super) fn set_theme(
        &mut self,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if theme::apply(cx, choice, window.appearance()).is_ok() {
            crate::prefs::save_theme(choice);
            self.map.update(cx, |m, cx| {
                m.cache.clear();
                cx.notify();
            });
            self.library.update(cx, |_, cx| cx.notify());
            self.overview.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }

    pub(super) fn next_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn cycle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = *cx.global::<Choice>();
        let mode = match c.mode {
            Mode::System => Mode::Dark,
            Mode::Dark => Mode::Light,
            Mode::Light => Mode::System,
        };
        self.set_theme(Choice { mode, ..c }, window, cx);
    }

    /// Compact, cosy, roomy, round again: the Library's and the List's rows together.
    pub(super) fn cycle_density(&mut self, cx: &mut Context<Self>) {
        let d = self.library.read(cx).density;
        let i = Density::ALL.iter().position(|x| *x == d).unwrap_or(0);
        let next = Density::ALL
            .get((i + 1) % Density::ALL.len())
            .copied()
            .unwrap_or_default();
        self.set_density(next, cx);
    }

    /// The rows' density, on every list.
    pub(super) fn set_density(&mut self, next: Density, cx: &mut Context<Self>) {
        self.library.update(cx, |l, cx| l.set_density(next, cx));
        self.overview.update(cx, |o, cx| o.set_density(next, cx));
        self.say(format!("Rows: {}", next.word()), cx);
    }

    /// Follows the system's appearance while the mode is System.
    pub(super) fn follow_system(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let c = *cx.global::<Choice>();
        if c.mode == Mode::System && c.mode.dark(window.appearance()) != tokens(cx).dark {
            self.set_theme(c, window, cx);
        }
    }

    pub(super) fn overlay_el(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
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
                overlays::help::sheet(&t, crate::VERSION).into_any_element(),
                0.,
                motion,
                "help",
            )),
            Some(Overlay::Bell(b)) => Some(self.bell_el(b.clone(), cx)),
            Some(o) => self.acting_overlay(o, cx),
            None => None,
        }
    }

    pub(super) fn palette_step(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            p.update(cx, |p, cx| p.step(by, cx));
        } else {
            cx.propagate();
        }
    }

    pub(super) fn palette_choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            p.update(cx, |p, cx| p.choose(window, cx));
        } else {
            cx.propagate();
        }
    }
}
