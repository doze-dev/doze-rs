//! Settings (PLAN.md §2.8 rows 82–84, §2.9 rows 87–88; M3 step 3.4), as the Settings and Doctor boards lay it out: a
//! short list at the left (General, Machine, About) and one part at a time.
//!
//! - General: the mode, the rows' density, the five colour themes drawn in the mode on screen, Open at login (a
//!   LaunchAgent through `doze_os::autostart`, in a folder the source names: a fixture's is a temporary one), Reopen
//!   the page I had, and where the prefs live.
//! - Machine: the setup part by part with each fix (`doze --json doctor`, plus whether a terminal finds doze), Repair
//!   through the system's administrator prompt, and what stacks left behind with a way to clear each. Looking only
//!   reads; acting needs a click, and a fixture acts on nothing.
//! - About: the version, and whether a newer doze is out (GitHub's latest release through the system's curl, kept six
//!   hours), with What's new and the command that updates.
//!
//! Every word and rule is `doze_desk::settings`'; this draws them and runs the slow parts off the UI thread.

mod about;
mod general;
mod left;
mod machine;
mod side;
mod work;

pub(crate) use work::doze_at;

use std::path::PathBuf;

use doze_desk::Hub;
use doze_desk::library::Doctor;
use doze_desk::settings::machine::{DozeAt, Part, parts};
use doze_desk::settings::update::Kept;
use doze_desk::settings::{Leftover, Machine, Update};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Window, div,
    prelude::*, px,
};

use crate::actions::{CloseOverlay, CopyReport, MoveDown, MoveUp};
use crate::facts::Facts;
use crate::theme::{Choice, Density};

/// A part of Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Section {
    #[default]
    General,
    Machine,
    About,
}

impl Section {
    pub(crate) const ALL: [Section; 3] = [Section::General, Section::Machine, Section::About];

    pub(crate) fn parse(word: &str) -> Option<Section> {
        match word {
            "general" => Some(Section::General),
            "machine" | "doctor" => Some(Section::Machine),
            "about" | "updates" => Some(Section::About),
            _ => None,
        }
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Machine => "Machine",
            Section::About => "About",
        }
    }
}

/// What Settings asks of the window.
#[derive(Debug, Clone)]
pub(crate) enum SettingsEvent {
    /// Says something, a toast; bad news in the bad colour.
    Say(String, bool),
    /// The theme chosen.
    Choose(Choice),
    /// The rows' density chosen.
    Density(Density),
    /// Back where the window was.
    Back,
    /// A privileged job, for the run sheet: shown first, run on a click.
    Run(doze_desk::settings::Job),
}

/// Made-up machine states, for fixtures and shots: a fixture's machine is not this one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Staged {
    pub(crate) doctor: Option<Doctor>,
    pub(crate) at: Option<DozeAt>,
    /// The app can ask for the administrator's password.
    pub(crate) admin: bool,
    pub(crate) machine: Option<Machine>,
    pub(crate) update: Option<Update>,
}

/// Settings.
pub(crate) struct SettingsView {
    pub(crate) hub: Hub,
    pub(crate) facts: Entity<Facts>,
    /// A fixture's source: nothing here acts on this machine.
    pub(crate) fixture: bool,
    pub(crate) section: Section,
    /// Where the login item lives (a fixture's is under a temporary folder).
    pub(crate) login_item: Option<PathBuf>,
    pub(crate) login: bool,
    /// The rows' density now (the Library's and the List's).
    pub(crate) density: Density,
    pub(crate) reopen: bool,
    pub(crate) parts: Vec<Part>,
    pub(crate) at: DozeAt,
    pub(crate) repairing: bool,
    pub(crate) left: Option<Vec<Leftover>>,
    pub(crate) looking: bool,
    /// What is being cleared (a leftover's id, or `all`), and which is asking before it deletes.
    pub(crate) busy: String,
    pub(crate) asking: String,
    /// The kinds shown whole.
    pub(crate) whole: Vec<&'static str>,
    pub(crate) update: Option<Update>,
    pub(crate) checking: bool,
    pub(crate) kept: Kept,
    pub(crate) staged: Staged,
    /// The part's scroll.
    pub(crate) scroll: gpui::ScrollHandle,
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub(crate) fn new(
        hub: Hub,
        facts: Entity<Facts>,
        fixture: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&facts, |v: &mut SettingsView, _, cx| v.refresh(cx))
            .detach();
        let login_item = work::login_item(fixture);
        let mut v = SettingsView {
            hub,
            facts,
            fixture,
            section: Section::General,
            login: login_item
                .as_deref()
                .is_some_and(doze_os::autostart::enabled),
            login_item,
            reopen: crate::prefs::page::reopen(),
            density: Density::parse(&crate::prefs::density()).unwrap_or_default(),
            parts: Vec::new(),
            at: DozeAt::default(),
            repairing: false,
            left: None,
            looking: false,
            busy: String::new(),
            asking: String::new(),
            whole: Vec::new(),
            update: None,
            checking: false,
            kept: Kept::default(),
            staged: Staged::default(),
            scroll: gpui::ScrollHandle::new(),
            focus: cx.focus_handle(),
        };
        v.at = work::doze_at(fixture);
        v.refresh(cx);
        v
    }

    /// Reads the machine's parts again from what doctor last said.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let stacks = self.hub.stacks();
        let doctor = match &self.staged.doctor {
            Some(d) => Some(d.clone()),
            None => self.facts.update(cx, |f, cx| f.doctor(&stacks, cx)),
        };
        let at = self.staged.at.clone().unwrap_or_else(|| self.at.clone());
        let next = doctor
            .map(|d| parts(&d, &at, crate::VERSION))
            .unwrap_or_default();
        if next != self.parts {
            self.parts = next;
            cx.notify();
        }
        // Every part reads the look (the side says what was left behind), not Machine alone.
        self.read_left(cx);
    }

    /// Shows a part; Machine looks for leftovers, About asks for the newest release (each when not known yet).
    pub(crate) fn show(&mut self, section: Section, force_update: bool, cx: &mut Context<Self>) {
        self.section = section;
        match section {
            Section::Machine if self.left.is_none() && !self.looking => self.look(cx),
            Section::About => self.check(force_update, cx),
            _ => {}
        }
        self.refresh(cx);
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let i = Section::ALL
            .iter()
            .position(|s| *s == self.section)
            .unwrap_or(0);
        let j = i.saturating_add_signed(by).min(Section::ALL.len() - 1);
        if let Some(s) = Section::ALL.get(j) {
            self.show(*s, false, cx);
        }
    }

    /// The app can ask for the administrator's password here.
    pub(crate) fn can_admin(&self) -> bool {
        if self.fixture {
            self.staged.admin
        } else {
            doze_os::privilege::admin_supported()
        }
    }

    fn copy_report(&mut self, cx: &mut Context<Self>) {
        let text = doze_desk::settings::machine::report(&self.parts);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        cx.emit(SettingsEvent::Say(
            "copied the report to the clipboard".into(),
            false,
        ));
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::tokens(cx);
        let body = match self.section {
            Section::General => general::page(&t, self, window, cx).into_any_element(),
            Section::Machine => machine::page(&t, self, cx).into_any_element(),
            Section::About => about::page(&t, self, cx).into_any_element(),
        };
        let keys: Vec<(String, &str)> = if self.section == Section::Machine {
            vec![("c".into(), "copy report"), ("esc".into(), "back")]
        } else {
            vec![
                (crate::kit::chip_of("Settings…"), "here"),
                ("esc".into(), "back"),
            ]
        };
        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus)
            .flex()
            .size_full()
            .min_h_0()
            .on_action(cx.listener(|v, _: &MoveDown, _, cx| v.step(1, cx)))
            .on_action(cx.listener(|v, _: &MoveUp, _, cx| v.step(-1, cx)))
            .on_action(cx.listener(|v, _: &CopyReport, _, cx| v.copy_report(cx)))
            .on_action(cx.listener(|v, _: &CloseOverlay, _, cx| {
                if v.asking.is_empty() {
                    cx.emit(SettingsEvent::Back);
                } else {
                    v.asking.clear();
                    cx.notify();
                }
            }))
            .child(side::side(&t, self, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id("settings-main")
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pb(px(16.))
                            .child(body),
                    )
                    .child(crate::kit::keys_bar(
                        &t,
                        &keys,
                        "from the menu bar, Home's Machine line, or ⌘,",
                    )),
            )
    }
}
