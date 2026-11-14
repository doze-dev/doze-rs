//! Acting (M2 steps 2.1, 2.2, 2.5, 2.6): one place every page asks to do something. A verb on a service asks first
//! when doze-desk says to ([`doze_desk::act::dialog_of`]), else goes to the daemon on its own connection; a stack's
//! action goes to the CLI after the checks before Up; a module's action asks for its input; a folder with no doze.hcl
//! is set up the way chosen. Everything runs off the UI thread; while it runs the stack or the service is busy, and
//! what it ends with is said (a toast, kept among the recent lines) or, for a stack, kept on its page.
//!
//! `Acts` is one entity for the app (a gpui global), so a page asks it and the window draws what it raises: a dialog,
//! the checks' sheet, the start sheet. It outlives the window, so recent lines survive a close.

mod check;
mod run;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use doze_desk::act::{Busy, Cli, Dialog, Failure, Notices, Preflight, StackAction, StartSheet};
use doze_desk::{Hub, StackKey};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task};

pub(crate) use check::Then;

/// What a dialog's buttons act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ask {
    /// The stack (none for setting a folder up).
    pub(crate) key: Option<StackKey>,
    pub(crate) dialog: Dialog,
}

/// The checks' sheet, and what goes ahead when it is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Check {
    pub(crate) key: StackKey,
    pub(crate) found: Preflight,
    pub(crate) then: Then,
    /// The ports the start declares (a stop is checked against them again).
    pub(crate) ports: Vec<u16>,
    /// What was asked to start, by name (for "Up without").
    pub(crate) names: Vec<String>,
}

/// What the window is asked to show.
#[derive(Debug, Clone)]
pub(crate) enum ActsEvent {
    Ask(Ask),
    Check(Check),
    Start(StackKey, StartSheet),
    /// A folder was set up: show its stack.
    Open(PathBuf),
}

/// The app's acting.
pub(crate) struct Acts {
    pub(crate) hub: Hub,
    /// The doze CLI for stack actions; `None` when no doze is found.
    pub(crate) cli: Option<Cli>,
    /// Where stopped stacks' shapes come from.
    pub(crate) mode: crate::facts::Mode,
    pub(crate) notices: Notices,
    /// The stacks with an action in flight.
    pub(crate) busy: HashMap<StackKey, Busy>,
    /// The last stack action that did not work, by stack, until dismissed.
    pub(crate) failed: HashMap<StackKey, Failure>,
    /// Services with a verb in flight.
    pub(crate) doing: HashMap<(StackKey, String), doze_control::Verb>,
    /// The Recent popover is open.
    pub(crate) recent: bool,
    /// A folder being set up, and the way (`init`, `add`, `empty`).
    pub(crate) writing: Option<(PathBuf, String)>,
    expiry: Option<Task<()>>,
}

impl EventEmitter<ActsEvent> for Acts {}

struct Handle(Entity<Acts>);

impl Global for Handle {}

/// The app's [`Acts`], once a window made it.
pub(crate) fn acts(cx: &App) -> Option<Entity<Acts>> {
    cx.try_global::<Handle>().map(|h| h.0.clone())
}

/// The app's [`Acts`], made on first use.
pub(crate) fn ensure(hub: &Hub, mode: &crate::facts::Mode, cx: &mut App) -> Entity<Acts> {
    if let Some(a) = acts(cx) {
        return a;
    }
    let cli = match mode {
        crate::facts::Mode::Live(doze) => doze.clone(),
        // The fixture puts a stand-in doze in its home (crate::fixture), so its stack actions run a real subprocess.
        crate::facts::Mode::Fixture => Some(hub.home().join("bin").join("doze")),
    }
    .map(|program| Cli {
        program,
        home: hub.home().to_path_buf(),
    });
    let a = cx.new(|_| Acts {
        hub: hub.clone(),
        cli,
        mode: mode.clone(),
        notices: Notices::default(),
        busy: HashMap::new(),
        failed: HashMap::new(),
        doing: HashMap::new(),
        recent: false,
        writing: None,
        expiry: None,
    });
    cx.set_global(Handle(a.clone()));
    a
}

impl Acts {
    /// Says `text`: a toast now, and a line kept.
    pub(crate) fn say(&mut self, text: impl Into<String>, bad: bool, cx: &mut Context<Self>) {
        self.say_id(text, bad, cx);
    }

    /// [`Acts::say`], returning the toast's id (to take it away when what it says is over).
    pub(crate) fn say_id(
        &mut self,
        text: impl Into<String>,
        bad: bool,
        cx: &mut Context<Self>,
    ) -> u64 {
        let text = text.into();
        #[cfg(feature = "fixtures")]
        crate::driver::toast(&text);
        let id = self
            .notices
            .say(text, bad, SystemTime::now(), Instant::now());
        self.schedule(cx);
        cx.notify();
        id
    }

    /// Takes a toast away.
    pub(crate) fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.notices.dismiss(id);
        self.schedule(cx);
        cx.notify();
    }

    /// Opens or closes the Recent popover.
    pub(crate) fn toggle_recent(&mut self, cx: &mut Context<Self>) {
        self.recent = !self.recent;
        cx.notify();
    }

    /// Wakes once, when the next toast's time is up: no timer runs while no toast shows.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let Some(wait) = self.notices.next_expiry(Instant::now()) else {
            self.expiry = None;
            return;
        };
        self.expiry = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |a, cx| {
                a.notices.expire(Instant::now());
                a.schedule(cx);
                cx.notify();
            });
        }));
    }

    /// What the stack is busy doing, if anything.
    pub(crate) fn busy_of(&self, key: &StackKey) -> Option<Busy> {
        self.busy.get(key).copied()
    }

    /// Puts a failed stack action away.
    pub(crate) fn dismiss_failure(&mut self, key: &StackKey, cx: &mut Context<Self>) {
        if self.failed.remove(key).is_some() {
            cx.notify();
        }
    }

    /// The start sheet for a stopped stack.
    pub(crate) fn choose_start(&mut self, key: &StackKey, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        if st.running {
            return;
        }
        let (mode, dir, name, last) = (
            self.mode.clone(),
            PathBuf::from(&st.dir),
            st.name.clone(),
            st.last_set.clone(),
        );
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let shape = cx
                .background_executor()
                .spawn(async move { check::shape_now(&mode, &dir) })
                .await;
            let sheet = StartSheet::new(&name, &shape.decls, &last);
            let _ = this.update(cx, |_, cx| cx.emit(ActsEvent::Start(key, sheet)));
        })
        .detach();
    }

    /// A stack's action, as its switch, its menu or a key asks: the checks before Up, the question before Down or
    /// Remove, then the CLI.
    pub(crate) fn stack(&mut self, key: &StackKey, action: StackAction, cx: &mut Context<Self>) {
        let Some(st) = self.hub.stack(key) else {
            return;
        };
        if let Some(b) = self.busy_of(key) {
            self.say(format!("{} is already {}", st.name, b.word()), true, cx);
            return;
        }
        match action {
            StackAction::Remove => self.ask(Some(key), doze_desk::act::forget_dialog(&st), cx),
            StackAction::Down => {
                let d = doze_desk::act::stop_dialog(&st);
                if d.needed {
                    self.ask(Some(key), d, cx);
                } else {
                    self.run_stack(key, StackAction::Down, cx);
                }
            }
            StackAction::Restart => self.run_stack(key, action, cx),
            up => self.check_up(key, up, cx),
        }
    }

    fn ask(&mut self, key: Option<&StackKey>, dialog: Dialog, cx: &mut Context<Self>) {
        cx.emit(ActsEvent::Ask(Ask {
            key: key.cloned(),
            dialog,
        }));
    }
}
