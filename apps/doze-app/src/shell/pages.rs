//! The page M3 adds to the window: Settings (its own list at the left). It comes by its key (⌘,), the menus
//! (Settings…, Check for Updates…), ⌘K, a link (`doze://settings`), the Library's Machine line and Needs you's machine
//! items. With Reopen the page I had on (the default), the window opens where it last was.

use gpui::{AnyElement, AppContext as _, Context, Entity, IntoElement, Window};

use super::{Root, Screen};
use crate::actions::{CheckUpdates, MachineSettings, RepairMachine, Settings, SettingsAt};
use crate::facts::Facts;
use crate::settings::{Section, SettingsEvent, SettingsView};

/// The pages, made on first visit; Settings is kept once made (its look at the machine and its update answer stay).
pub(crate) struct Pages {
    pub(crate) facts: Entity<Facts>,
    pub(crate) fixture: bool,
    pub(crate) settings: Option<Entity<SettingsView>>,
    /// The last privileged job's sheet, kept while it runs even when hidden.
    pub(crate) admin: Option<Entity<crate::overlays::admin::AdminView>>,
}

impl Pages {
    pub(crate) fn new(facts: Entity<Facts>, fixture: bool) -> Pages {
        Pages {
            facts,
            fixture,
            settings: None,
            admin: None,
        }
    }
}

impl Root {
    /// The window's M3 commands.
    pub(super) fn on_pages(&mut self, d: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use gpui::InteractiveElement as _;
        d.on_action(cx.listener(|this, _: &Settings, window, cx| {
            this.open_settings(None, false, window, cx);
        }))
        .on_action(cx.listener(|this, s: &SettingsAt, window, cx| {
            this.open_settings(Section::parse(&s.section), false, window, cx);
        }))
        .on_action(cx.listener(|this, _: &MachineSettings, window, cx| {
            this.open_settings(Some(Section::Machine), false, window, cx);
        }))
        .on_action(cx.listener(|this, _: &CheckUpdates, window, cx| {
            this.open_settings(Some(Section::About), true, window, cx);
        }))
        // Repair from anywhere (Needs you, a stack's setup banner, the Library's machine line): the run sheet over
        // the page it was asked from, as Go ran it in place.
        .on_action(cx.listener(|this, _: &RepairMachine, window, cx| {
            let s = this.settings_view(window, cx);
            s.update(cx, |s, cx| {
                s.refresh(cx);
                s.repair(cx);
            });
        }))
    }

    /// Settings, at a part when one is named; About asks GitHub afresh when `force`.
    pub(crate) fn open_settings(
        &mut self,
        section: Option<Section>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go(Screen::Settings, window, cx);
        if let Some(s) = self.pages.settings.clone() {
            let density = self.library.read(cx).density;
            s.update(cx, |s, cx| {
                s.density = density;
                let at = section.unwrap_or(s.section);
                s.show(at, force, cx);
            });
        }
    }

    /// Makes Settings once, and gives it the keys.
    pub(super) fn enter_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.settings_view(window, cx);
        let f = s.read(cx).focus.clone();
        window.focus(&f, cx);
    }

    /// Settings, made once (without going there).
    pub(crate) fn settings_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SettingsView> {
        if let Some(s) = &self.pages.settings {
            return s.clone();
        }
        let (hub, facts, fixture) = (
            self.hub.clone(),
            self.pages.facts.clone(),
            self.pages.fixture,
        );
        let v = cx.new(|cx| SettingsView::new(hub, facts, fixture, cx));
        cx.subscribe_in(
            &v,
            window,
            |this, _, ev: &SettingsEvent, window, cx| match ev {
                SettingsEvent::Say(s, bad) => {
                    let (s, bad) = (s.clone(), *bad);
                    this.acts.update(cx, |a, cx| a.say(s, bad, cx));
                }
                SettingsEvent::Choose(c) => this.set_theme(*c, window, cx),
                SettingsEvent::Density(d) => this.set_density(*d, cx),
                SettingsEvent::Back => this.leave_settings(window, cx),
                SettingsEvent::Run(job) => this.open_admin(job.clone(), window, cx),
            },
        )
        .detach();
        self.pages.settings = Some(v.clone());
        v
    }

    /// Esc on Settings: back where the window was, or the Library.
    fn leave_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = self.hist.before().filter(|s| *s != Screen::Settings);
        self.go(back.unwrap_or(Screen::Library), window, cx);
    }

    /// The keys go back to Settings (an overlay closed over it).
    pub(super) fn pages_focus(&self, cx: &gpui::App) -> gpui::FocusHandle {
        let page = (self.screen == Screen::Settings)
            .then(|| {
                self.pages
                    .settings
                    .as_ref()
                    .map(|s| s.read(cx).focus.clone())
            })
            .flatten();
        page.unwrap_or_else(|| self.library.read(cx).focus.clone())
    }

    /// The page for Settings, when it shows.
    pub(super) fn pages_el(&self) -> Option<AnyElement> {
        (self.screen == Screen::Settings)
            .then(|| {
                self.pages
                    .settings
                    .clone()
                    .map(IntoElement::into_any_element)
            })
            .flatten()
    }

    /// A batch from the hub, for Settings (the stacks moved).
    pub(super) fn apply_pages(&mut self, batch: &doze_desk::Batch, cx: &mut Context<Self>) {
        if self.screen == Screen::Settings
            && !batch.hub.is_empty()
            && let Some(s) = &self.pages.settings
        {
            s.update(cx, |s, cx| s.refresh(cx));
        }
    }
}
