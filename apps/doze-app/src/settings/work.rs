//! Settings' slow parts, off the UI thread: the login item, Repair, the look for leftovers and clearing them, the
//! update check. A fixture's source acts on nothing: its machine is made up (`Staged`), and its buttons say so.

use std::path::{Path, PathBuf};

use doze_desk::settings::leftovers::{self, Cleared};
use doze_desk::settings::repair::{self, Fail, Hands};
use doze_desk::settings::update::{self, Update};
use doze_desk::settings::{DozeAt, SystemHands};
use gpui::Context;

use super::{SettingsEvent, SettingsView};

/// Where the login item lives: the person's own folder for a live source; for a fixture, a temporary folder of its
/// own, so a shot or a test never registers the real login item.
pub(super) fn login_item(fixture: bool) -> Option<PathBuf> {
    let home = if fixture {
        std::env::temp_dir().join(format!("doze-app-login-{}", std::process::id()))
    } else {
        PathBuf::from(std::env::var_os("HOME")?)
    };
    let config = (!fixture)
        .then(|| std::env::var_os("XDG_CONFIG_HOME"))
        .flatten()
        .map(PathBuf::from);
    doze_os::autostart::path_in(&home, config.as_deref())
}

/// Where the `doze` is: a fixture's is where Homebrew puts it.
pub(super) fn doze_at(fixture: bool) -> DozeAt {
    if fixture {
        return DozeAt {
            on_path: Some("/opt/homebrew/bin/doze".into()),
            elsewhere: None,
        };
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    DozeAt::find(std::env::var_os("PATH").as_deref(), home.as_deref())
}

/// The doze the app's own login item starts: this program, in the menu bar only.
fn this_app() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("doze-app"))
}

impl SettingsView {
    /// Open at login, on or off.
    pub(crate) fn set_login(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.login_item.clone() else {
            cx.emit(SettingsEvent::Say(
                "opening at login is not supported here".into(),
                true,
            ));
            return;
        };
        let on = !self.login;
        match doze_os::autostart::set(&path, on, &this_app(), &["--menu-bar"]) {
            Ok(()) => self.login = on,
            Err(e) => cx.emit(SettingsEvent::Say(
                format!("could not change the login item: {e}"),
                true,
            )),
        }
        cx.notify();
    }

    fn hands(&self) -> Option<SystemHands> {
        Some(SystemHands {
            doze: crate::facts::find_doze()?,
            home: self.hub.home().to_path_buf(),
        })
    }

    /// Repair: the steps the failing parts need, behind the system's prompt; doctor looks again after.
    pub(crate) fn repair(&mut self, cx: &mut Context<Self>) {
        if self.repairing {
            return;
        }
        if self.fixture {
            cx.emit(SettingsEvent::Say(
                "a fixture repairs nothing on this machine".into(),
                true,
            ));
            return;
        }
        let Some(mut hands) = self.hands() else {
            cx.emit(SettingsEvent::Say(
                "this app cannot find doze to run the setup: copy the fix into a terminal".into(),
                true,
            ));
            return;
        };
        let steps = repair::steps(
            &self.hub.stacks(),
            &self.parts,
            self.at.elsewhere.as_deref(),
        );
        let home = self.hub.home().to_path_buf();
        self.repairing = true;
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            let mut prune = |h: &mut dyn Hands| prune_routes(&home, h);
            repair::run(&steps, &mut hands, &mut prune)
        });
        cx.spawn(async move |this, cx| {
            let out = job.await;
            let _ = this.update(cx, |v, cx| {
                v.repairing = false;
                v.repaired = Some(out);
                v.facts.update(cx, |f, cx| f.forget_doctor(cx));
                v.refresh(cx);
            });
        })
        .detach();
    }

    /// Looks again for what stacks left behind: a fixture's made-up machine, or this one through the window's one
    /// look (`Facts::leftovers`, which Needs you counts from; reading only).
    pub(crate) fn look(&mut self, cx: &mut Context<Self>) {
        if self.fixture {
            self.left = Some(
                self.staged
                    .machine
                    .as_ref()
                    .map(leftovers::find)
                    .unwrap_or_default(),
            );
            cx.notify();
            return;
        }
        self.facts.update(cx, |f, _| f.forget_leftovers());
        self.refresh(cx);
    }

    /// What the window's look last found (live only); starts a look when it is old.
    pub(super) fn read_left(&mut self, cx: &mut Context<Self>) {
        if self.fixture {
            return;
        }
        let (home, stacks) = (self.hub.home().to_path_buf(), self.hub.stacks());
        let (left, looking) = self.facts.update(cx, |f, cx| {
            let l = f.leftovers(&home, &stacks, cx);
            (l, f.looking_left())
        });
        if left != self.left || looking != self.looking {
            self.left = left;
            self.looking = looking;
            cx.notify();
        }
    }

    /// Clears one leftover by its id, or `all`; looking again first, and again after.
    pub(crate) fn clear(&mut self, id: &str, cx: &mut Context<Self>) {
        self.asking.clear();
        if self.fixture {
            cx.emit(SettingsEvent::Say(
                "a fixture clears nothing on this machine".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let Some(mut hands) = self.hands() else {
            return;
        };
        self.busy = id.to_owned();
        cx.notify();
        let (id, home, stacks, admin) = (
            id.to_owned(),
            self.hub.home().to_path_buf(),
            self.hub.stacks(),
            self.can_admin(),
        );
        let job = cx.background_executor().spawn(async move {
            let m = leftovers::gather(&home, &stacks, admin);
            if id == "all" {
                leftovers::clear_all(&m, &mut hands)
            } else {
                leftovers::clear(&id, &m, &mut hands)
            }
        });
        cx.spawn(async move |this, cx| {
            let Cleared { said, bad } = job.await;
            let _ = this.update(cx, |v, cx| {
                v.busy.clear();
                cx.emit(SettingsEvent::Say(said, bad));
                v.look(cx);
            });
        })
        .detach();
    }

    /// Whether a newer doze is out: the answer kept (six hours), or GitHub's now.
    pub(crate) fn check(&mut self, force: bool, cx: &mut Context<Self>) {
        let now = self.hub.now();
        let zone = crate::instance::zone();
        if let Some(u) = &self.staged.update {
            self.update = Some(u.clone());
            cx.notify();
            return;
        }
        if self.fixture {
            // A fixture asks nobody (Go's did the same).
            self.update = Some(Update {
                current: current().trim_start_matches('v').to_owned(),
                dev: !update::released(current()),
                ..Update::default()
            });
            cx.notify();
            return;
        }
        if let Some(u) = self.kept.get(now, force) {
            self.update = Some(u);
            cx.notify();
            return;
        }
        if self.checking {
            return;
        }
        self.checking = true;
        cx.notify();
        let job = cx.background_executor().spawn(async move {
            update::update_of(current(), update::fetch(update::LATEST_URL), now, zone)
        });
        cx.spawn(async move |this, cx| {
            let u: Update = job.await;
            let _ = this.update(cx, |v, cx| {
                v.checking = false;
                v.kept.keep(now, u.clone());
                v.update = Some(u);
                cx.notify();
            });
        })
        .detach();
    }
}

/// This doze's version for the update check: the release build's, or "dev".
fn current() -> &'static str {
    option_env!("DOZE_VERSION").unwrap_or("dev")
}

/// Removes every stale route behind one prompt, then forgets them (a repair's step when no stack runs doze trust).
fn prune_routes(home: &Path, hands: &mut dyn Hands) -> Result<(), Fail> {
    let m = leftovers::gather(home, &[], true);
    let script = leftovers::route_script(&m.routes);
    if script.is_empty() {
        return Ok(());
    }
    hands.admin("remove stale routes", &script)?;
    leftovers::forget_routes(home, &m.routes).map_err(|e| Fail::Failed(e.to_string()))
}
