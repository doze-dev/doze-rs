//! The pages M3 adds to the window: Activity (a stack's, full width, as Go placed it) and Settings (its own list at
//! the left). They come by their keys (⇧A, ⌘,), the menus (Activity, Settings…, Check for Updates…), ⌘K, a link
//! (`doze://settings`), the Library's Machine line, Needs you's machine items and the stack page's Activity button.
//! With Reopen the page I had on (the default), the window opens where it last was.

use doze_desk::StackKey;
use gpui::{AnyElement, AppContext as _, Context, Entity, IntoElement, Window};

use super::{Root, Screen};
use crate::actions::{
    Activity, CheckUpdates, MachineSettings, RepairMachine, Settings, SettingsAt,
};
use crate::activity::{ActivityEvent, ActivityView};
use crate::facts::Facts;
use crate::settings::{Section, SettingsEvent, SettingsView};

/// The pages, made on first visit; Settings is kept once made (its look at the machine and its update answer stay).
pub(crate) struct Pages {
    pub(crate) facts: Entity<Facts>,
    pub(crate) fixture: bool,
    pub(crate) activity: Option<Entity<ActivityView>>,
    pub(crate) settings: Option<Entity<SettingsView>>,
    /// The last privileged job's sheet, kept while it runs even when hidden.
    pub(crate) admin: Option<Entity<crate::overlays::admin::AdminView>>,
}

impl Pages {
    pub(crate) fn new(facts: Entity<Facts>, fixture: bool) -> Pages {
        Pages {
            facts,
            fixture,
            activity: None,
            settings: None,
            admin: None,
        }
    }
}

impl Root {
    /// The window's M3 commands.
    pub(super) fn on_pages(&mut self, d: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use gpui::InteractiveElement as _;
        d.on_action(cx.listener(|this, _: &Activity, window, cx| this.open_activity(window, cx)))
            .on_action(cx.listener(|this, _: &Settings, window, cx| {
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

    /// Activity of the stack shown, or of the first running stack from the Library.
    pub(crate) fn open_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.key().cloned().or_else(|| {
            let stacks = self.hub.stacks();
            stacks
                .iter()
                .find(|s| s.running)
                .or(stacks.first())
                .map(|s| s.key.clone())
        });
        match key {
            Some(k) => self.go(Screen::Activity(k), window, cx),
            None => self.say("Activity is a stack's: no stack is here yet.", cx),
        }
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

    /// Makes the Activity page for [`Screen::Activity`], unless it shows that stack already.
    pub(super) fn enter_activity(
        &mut self,
        key: &StackKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same = self
            .pages
            .activity
            .as_ref()
            .is_some_and(|a| a.read(cx).key == *key);
        if !same {
            let (hub, k) = (self.hub.clone(), key.clone());
            let v = cx.new(|cx| ActivityView::new(hub, k, cx));
            cx.subscribe_in(
                &v,
                window,
                |this, _, ev: &ActivityEvent, window, cx| match ev {
                    ActivityEvent::Back => {
                        if let Some(k) = this.key().cloned() {
                            this.go(Screen::Stack(k), window, cx);
                        }
                    }
                },
            )
            .detach();
            self.pages.activity = Some(v);
        }
        if let Some(a) = &self.pages.activity {
            let f = a.read(cx).focus.clone();
            window.focus(&f, cx);
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

    /// The keys go back to Activity or Settings (an overlay closed over it).
    pub(super) fn pages_focus(&self, cx: &gpui::App) -> gpui::FocusHandle {
        let page = match self.screen {
            Screen::Activity(_) => self
                .pages
                .activity
                .as_ref()
                .map(|a| a.read(cx).focus.clone()),
            Screen::Settings => self
                .pages
                .settings
                .as_ref()
                .map(|s| s.read(cx).focus.clone()),
            _ => None,
        };
        page.unwrap_or_else(|| self.library.read(cx).focus.clone())
    }

    /// The page for Activity or Settings, when one shows.
    pub(super) fn pages_el(&self) -> Option<AnyElement> {
        match self.screen {
            Screen::Activity(_) => self
                .pages
                .activity
                .clone()
                .map(IntoElement::into_any_element),
            Screen::Settings => self
                .pages
                .settings
                .clone()
                .map(IntoElement::into_any_element),
            _ => None,
        }
    }

    /// A batch from the hub, for Activity (the journal moved) and Settings (the stacks moved).
    pub(super) fn apply_pages(&mut self, batch: &doze_desk::Batch, cx: &mut Context<Self>) {
        match &self.screen {
            Screen::Activity(_) => {
                if let Some(a) = &self.pages.activity {
                    a.update(cx, |a, cx| a.apply(batch, cx));
                }
            }
            Screen::Settings if !batch.hub.is_empty() => {
                if let Some(s) = &self.pages.settings {
                    s.update(cx, |s, cx| s.refresh(cx));
                }
            }
            _ => {}
        }
    }
}
