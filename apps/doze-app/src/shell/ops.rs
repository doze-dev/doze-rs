//! The root's overlays (⌘K and the help sheet), the theme and the density.

use gpui::{AppContext as _, Context, IntoElement, Window};

use super::{Root, Screen};
use crate::actions::{OpenPalette, ShowHelp};
use crate::overlays::palette::{Palette, PaletteEvent, Pick};
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
        let cards: Vec<(String, String)> = match &self.screen {
            Screen::Stack(_) | Screen::Instance(..) | Screen::Logs(_) => {
                crate::overview::all_rows(&self.overview.read(cx).home)
                    .into_iter()
                    .map(|r| (r.name.clone(), format!("{} · {}", r.look.word, r.because)))
                    .collect()
            }
            Screen::Library => Vec::new(),
        };
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
                (Pick::Card(_), Some(name)) => this.open_instance(&name, window, cx),
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

    /// Closes the innermost thing open: an overlay, else what the page inside holds or the page itself, else the Map's
    /// find box or its selection, else the stack's page itself (back to Home).
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on_map =
            matches!(self.screen, Screen::Stack(_)) && self.overview.read(cx).view == View::Map;
        if self.overlay.take().is_some() {
            if self.focus_inside(window, cx) {
                cx.notify();
                return;
            }
            let f = match &self.screen {
                Screen::Stack(_) if on_map => self.map.read(cx).focus.clone(),
                Screen::Stack(_) => self.overview.read(cx).focus.clone(),
                Screen::Library | Screen::Instance(..) | Screen::Logs(_) => {
                    self.library.read(cx).focus.clone()
                }
            };
            window.focus(&f, cx);
            cx.notify();
            return;
        }
        if self.escape_inside(window, cx) {
            return;
        }
        let map_held = on_map && {
            let m = self.map.read(cx);
            m.lit.is_some() || m.search.is_some()
        };
        if map_held {
            self.map.update(cx, |m, cx| m.escape(window, cx));
        } else if matches!(self.screen, Screen::Stack(_)) {
            // Nothing open, nothing held: esc goes home, as the Go page's did.
            self.go(Screen::Library, window, cx);
        }
    }

    fn set_theme(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
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
                90.,
                motion,
                "help",
            )),
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

    pub(super) fn palette_choose(&mut self, cx: &mut Context<Self>) {
        if let Some(Overlay::Palette(p)) = &self.overlay {
            p.update(cx, |p, cx| p.choose(cx));
        } else {
            cx.propagate();
        }
    }
}
