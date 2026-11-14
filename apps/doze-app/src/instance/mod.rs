//! An instance's page (PLAN.md §2.4): the head with the switch and its verbs, the tabs, and under them the overview's
//! cards, the logs, a task's runs, a web console's way out, or a module's console tab. What it shows is
//! `doze_desk::page::instance`, recomputed from the hub only when its stack changed; the meters and the logs stream
//! for as long as the page is open (the watches it holds), and a module's console too.

mod asleep;
mod clash;
pub(crate) mod head;
mod overview;
pub(crate) mod side;
mod snippets;
mod task;
pub(crate) mod ui;
mod verbs;

use std::time::{Duration, Instant};

use doze_control::Topic;
use doze_desk::page::instance::{InstancePage, instance_page};
use doze_desk::page::time::Zone;
use doze_desk::{Batch, ConsoleWatch, Hub, Parts, StackKey, Watch, ports};
use doze_display as words;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Window, div, prelude::*, px,
};

use crate::actions::{
    CopyAddress, NextTab, OpenAddress, PinIt, PrevTab, RestartIt, RunTask, ShowLogs, SleepIt,
    WakeIt,
};
use crate::console::ConsoleTabView;
use crate::logs::{LogsMode, LogsView};
use crate::theme::{tokens, ty};

/// What the page asks the window to do.
#[derive(Debug, Clone)]
pub(crate) enum InstanceEvent {
    /// Open another instance of the stack (a need's chip, a sidebar row).
    Open(String),
    /// Go home (the port clash's way out when the holder is another stack).
    Home,
    /// Say something at the foot.
    Say(SharedString),
}

/// The person's zone now: the clock times on the page.
pub(crate) fn zone() -> Zone {
    use chrono::Offset as _;
    Zone(chrono::Local::now().offset().fix().local_minus_utc())
}

/// An instance's page.
pub(crate) struct InstanceView {
    pub(crate) hub: Hub,
    pub(crate) key: StackKey,
    pub(crate) name: String,
    pub(crate) tab: String,
    pub(crate) page: InstancePage,
    /// The stack's folder, for Edit doze.hcl.
    pub(crate) dir: String,
    logs: Option<Entity<LogsView>>,
    console: Option<Entity<ConsoleTabView>>,
    /// What holds the port its crash names, once looked up.
    pub(crate) held: Option<ports::Holder>,
    held_port: Option<u16>,
    /// The snippet language shown.
    pub(crate) lang: usize,
    /// The task run whose output is shown, by its number among the runs kept.
    pub(crate) run_shown: Option<usize>,
    /// Which copy link says "Copied", and since when.
    pub(crate) copied: Option<(String, Instant)>,
    ticking: bool,
    /// The stack's daemon is not answering, or has gone quiet: the banner's words.
    stale: Option<String>,
    /// Its stack's doze.hcl does not load: `doze.hcl does not load (line 9)` (set by the window, from the facts).
    pub(crate) broken: Option<String>,
    /// Its meters are watched, so a frame is due every second: the page reads the hub again each second.
    metered: bool,
    _watches: (Watch, Watch, Option<ConsoleWatch>),
    pub(crate) focus: FocusHandle,
}

impl EventEmitter<InstanceEvent> for InstanceView {}

impl InstanceView {
    pub(crate) fn new(hub: Hub, key: StackKey, name: &str, cx: &mut Context<Self>) -> Self {
        let watches = (
            hub.watch(&key, Topic::Stats),
            hub.watch(&key, Topic::Logs),
            None,
        );
        let mut v = InstanceView {
            hub,
            key,
            name: name.to_owned(),
            tab: String::new(),
            page: InstancePage::default(),
            dir: String::new(),
            logs: None,
            console: None,
            held: None,
            held_port: None,
            lang: 0,
            run_shown: None,
            copied: None,
            ticking: false,
            stale: None,
            broken: None,
            metered: false,
            _watches: watches,
            focus: cx.focus_handle(),
        };
        v.refresh(cx);
        if let Some(t) = v.page.tabs.first() {
            v.tab = t.id.clone();
        }
        v.open_tab(cx);
        v
    }

    /// A batch from the hub: the page again if its stack changed (the logs alone matter only to a crash's last
    /// lines and a task's runs), then the open tab's own part.
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        let parts = batch.parts(&self.key);
        if parts.is_empty() {
            return;
        }
        let logs_matter = self.page.crash.is_some() || self.page.is_task();
        if parts.intersects(
            Parts::INSTANCES
                | Parts::STATS
                | Parts::JOURNAL
                | Parts::CONSOLE
                | Parts::META
                | Parts::CONN,
        ) || (logs_matter && parts.contains(Parts::LOGS))
        {
            self.refresh(cx);
        }
        if let Some(l) = &self.logs {
            l.update(cx, |l, cx| l.apply(batch, cx));
        }
        if let Some(c) = &self.console {
            c.update(cx, |c, cx| c.apply(batch, cx));
        }
    }

    /// Reads the page from the hub; redraws only when it changed.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(&self.key) else {
            return;
        };
        let now = self.hub.now();
        // The daemon not answering, or gone quiet: said over the page, the head faded (Go `app.tsx`, `.head.faded`).
        let b = doze_desk::overview::banner(&st, now);
        let stale = (!b.ok && !b.text.is_empty()).then_some(b.text);
        if stale != self.stale {
            self.stale = stale;
            cx.notify();
        }
        self.metered = st.running && st.metered;
        self.tick(cx);
        let page = self
            .hub
            .with_logs(&self.key, |r| {
                instance_page(&st, &self.name, Some(r), now, zone())
            })
            .unwrap_or_else(|| instance_page(&st, &self.name, None, now, zone()));
        self.dir.clone_from(&st.dir);
        // A console is a module's alone (the owner's rule): no other kind's page subscribes to one.
        let has_console = st
            .instance(&self.name)
            .is_some_and(|v| v.has_console && v.kind == "module");
        if has_console && self._watches.2.is_none() {
            self._watches.2 = Some(self.hub.watch_console(&self.key, &self.name));
        }
        if page == self.page {
            return;
        }
        let port = page.crash.as_ref().and_then(|c| c.port);
        self.page = page;
        if self.tab == words::TAB_LAST_RUN {
            self.open_tab(cx);
        }
        if !self.page.tabs.iter().any(|t| t.id == self.tab)
            && let Some(t) = self.page.tabs.first()
        {
            self.tab = t.id.clone();
            self.open_tab(cx);
        }
        self.look_up_holder(port, cx);
        self.tick(cx);
        cx.notify();
    }

    /// Repaints once a second while a countdown shows (dozing, the next try), and only then.
    fn tick(&mut self, cx: &mut Context<Self>) {
        let counting = self.page.dozing.is_some()
            || self.metered
            || self
                .page
                .crash
                .as_ref()
                .is_some_and(|c| !c.next_in.is_empty());
        if !counting || self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let more = this.update(cx, |v, cx| {
                    v.ticking = false;
                    v.refresh(cx);
                    let more = v.page.dozing.is_some()
                        || v.metered
                        || v.page.crash.as_ref().is_some_and(|c| !c.next_in.is_empty());
                    v.ticking = more;
                    more
                });
                if !matches!(more, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// Makes the open tab's view: the logs (or a task's runs), a module's console.
    fn open_tab(&mut self, cx: &mut Context<Self>) {
        let (hub, key, name) = (self.hub.clone(), self.key.clone(), self.name.clone());
        let want = if self.tab == words::TAB_LOGS {
            Some(LogsMode::Tab)
        } else if self.tab == words::TAB_LAST_RUN && !self.page.runs.is_empty() {
            Some(LogsMode::Bare)
        } else {
            None
        };
        let have = self.logs.as_ref().map(|l| l.read(cx).mode);
        if want != have {
            self.logs = want.map(|mode| cx.new(|cx| LogsView::new(hub, key, &name, mode, cx)));
        }
        if self.tab.starts_with("console:") {
            let tab = self.tab.clone();
            let c = cx
                .new(|cx| ConsoleTabView::new(self.hub.clone(), self.key.clone(), &name, &tab, cx));
            self.console = Some(c);
        } else {
            self.console = None;
        }
        self.sync_run(cx);
    }

    /// A task's output follows the run picked in the runs table (the newest until another is).
    pub(crate) fn sync_run(&mut self, cx: &mut Context<Self>) {
        let (Some(l), true) = (&self.logs, self.tab == words::TAB_LAST_RUN) else {
            return;
        };
        let run = self
            .page
            .runs
            .iter()
            .find(|r| Some(r.seq) == self.run_shown)
            .or(self.page.runs.first())
            .map_or(0, |r| r.log);
        l.update(cx, |l, cx| l.pin_run(run, cx));
    }

    pub(crate) fn set_tab(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab == id {
            return;
        }
        id.clone_into(&mut self.tab);
        self.open_tab(cx);
        self.focus_tab(window, cx);
        cx.notify();
    }

    /// Focus goes to the open tab's own view, so its keys work at once.
    pub(crate) fn focus_tab(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(l) = &self.logs {
            let f = l.read(cx).focus.clone();
            window.focus(&f, cx);
        } else if let Some(c) = &self.console {
            let f = c.read(cx).focus.clone();
            window.focus(&f, cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }

    fn step_tab(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.page.tabs.len() as isize;
        let i = self
            .page
            .tabs
            .iter()
            .position(|t| t.id == self.tab)
            .unwrap_or(0) as isize;
        if let Some(t) = self.page.tabs.get(((i + by).rem_euclid(n.max(1))) as usize) {
            let id = t.id.clone();
            self.set_tab(&id, window, cx);
        }
    }

    /// Copies `what`, and has the link named `id` say so for a moment.
    pub(crate) fn copy(&mut self, id: &str, what: &str, cx: &mut Context<Self>) {
        crate::outward::copy(cx, what);
        self.copied = Some((id.to_owned(), Instant::now()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1200))
                .await;
            let _ = this.update(cx, |v, cx| {
                if v.copied
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() >= Duration::from_millis(1100))
                {
                    v.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(crate) fn is_copied(&self, id: &str) -> bool {
        self.copied.as_ref().is_some_and(|(c, _)| c == id)
    }

    pub(crate) fn say(&self, words: impl Into<SharedString>, cx: &mut Context<Self>) {
        cx.emit(InstanceEvent::Say(words.into()));
    }

    /// Esc: the logs tab lets go of what it holds first (the filter box, picked lines).
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match &self.logs {
            Some(l) => l.update(cx, |l, cx| l.escape(window, cx)),
            None => false,
        }
    }
}

impl Focusable for InstanceView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for InstanceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        if self.page.missing {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(ty::LG_UP))
                        .text_color(t.bright)
                        .child(ui::text(&format!("Nothing called {}.", self.name))),
                )
                .child(div().text_color(t.dim).child(ui::text(&format!(
                    "{} does not declare it any more. The sidebar lists what it does.",
                    self.page.stack
                ))))
                .into_any_element();
        }
        let body = self.body(window, cx);
        let console = self.tab.starts_with("console:");
        div()
            .id("instance")
            // No ligatures, as Go's `font-variant-ligatures: none`: a mono face with ligatures would join the `//` of a URL.
            .font_features(gpui::FontFeatures::disable_ligatures())
            .key_context("Instance")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .on_action(cx.listener(|v, _: &RestartIt, w, cx| v.restart(w, cx)))
            .on_action(cx.listener(|v, _: &SleepIt, w, cx| v.sleep(w, cx)))
            .on_action(cx.listener(|v, _: &WakeIt, w, cx| v.wake(w, cx)))
            .on_action(cx.listener(|v, _: &PinIt, w, cx| v.pin(w, cx)))
            .on_action(cx.listener(|v, _: &RunTask, w, cx| v.run_task(w, cx)))
            .on_action(cx.listener(|v, _: &OpenAddress, _, cx| v.open_address(cx)))
            .on_action(cx.listener(|v, _: &CopyAddress, _, cx| v.copy_address(cx)))
            .on_action(cx.listener(|v, _: &ShowLogs, w, cx| v.set_tab(words::TAB_LOGS, w, cx)))
            .on_action(cx.listener(|v, _: &NextTab, w, cx| v.step_tab(1, w, cx)))
            .on_action(cx.listener(|v, _: &PrevTab, w, cx| v.step_tab(-1, w, cx)))
            .children(
                self.stale
                    .as_deref()
                    .map(|s| crate::overview::stale_banner(&t, s)),
            )
            .child(
                div()
                    .when(self.stale.is_some(), |d| d.opacity(0.55))
                    .child(head::head(&t, self, cx)),
            )
            .children(self.broken.as_deref().map(|b| {
                let home = cx.listener(|_, _, _, cx| cx.emit(InstanceEvent::Home));
                crate::overview::broken_banner(&t, (b, &self.page.stack), Box::new(home))
            }))
            .child(head::tabs(&t, self, cx))
            .child(body)
            .when(!console && self.tab != words::TAB_LOGS, |d| {
                d.child(head::keys(&t, self))
            })
            .into_any_element()
    }
}

impl InstanceView {
    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = tokens(cx);
        match self.tab.as_str() {
            words::TAB_LOGS => self
                .logs
                .clone()
                .map(|l| l.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            words::TAB_LAST_RUN => task::task(&t, self, cx),
            words::TAB_WEB => overview::web_console(&t, self, cx),
            words::TAB_OVERVIEW if self.page.is_task() => task::task(&t, self, cx),
            words::TAB_OVERVIEW if self.page.look == Some(words::Look::Asleep) => {
                overview::asleep(&t, self, cx)
            }
            words::TAB_OVERVIEW => overview::overview(&t, self, window, cx),
            _ => self
                .console
                .clone()
                .map(|c| c.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
        }
    }

    /// The logs view a task's last run shows its output in.
    pub(crate) fn output(&self) -> Option<Entity<LogsView>> {
        self.logs.clone()
    }
}
