//! Settings' slow parts, off the UI thread: the login item, Repair, the look for leftovers and clearing them, the
//! update check. A fixture's source acts on nothing: its machine is made up (`Staged`), and its buttons say so.

use std::path::PathBuf;

use doze_desk::settings::Job;
use doze_desk::settings::leftovers::{self, Cleared};
use doze_desk::settings::repair;
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
pub(crate) fn doze_at(fixture: bool) -> DozeAt {
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
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("doze-app"));
    stable(&exe, std::env::var_os("PATH").as_deref())
}

/// The path to write into the login item (Go `autostart.Stable`): the program itself inside an app bundle, else the
/// same program found on the `PATH` when it is the same file, so an upgrade into a new folder (Homebrew's Cellar) does
/// not leave the item pointing at a version that is gone.
fn stable(exe: &std::path::Path, path: Option<&std::ffi::OsStr>) -> PathBuf {
    if exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        return exe.to_path_buf();
    }
    let Some(name) = exe.file_name() else {
        return exe.to_path_buf();
    };
    let on_path = path
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| p.is_file());
    match on_path {
        Some(p) if std::fs::canonicalize(&p).ok() == std::fs::canonicalize(exe).ok() => {
            std::path::absolute(&p).unwrap_or(p)
        }
        _ => exe.to_path_buf(),
    }
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
            Ok(()) => {
                self.login = on;
                cx.emit(SettingsEvent::Say(
                    if on {
                        "doze will start at login, in the menu bar"
                    } else {
                        "doze will not start at login"
                    }
                    .into(),
                    false,
                ));
            }
            Err(e) => cx.emit(SettingsEvent::Say(
                format!("could not change the login item: {e}"),
                true,
            )),
        }
        cx.notify();
    }

    /// The machine's hands for a clear that needs no root: stopping a process, deleting a folder (no CLI involved, so
    /// a missing `doze` does not stop it), refusing anything that would ask for the password.
    fn hands(&self) -> NoRoot {
        let cli = doze_desk::settings::Cli::from_env()
            .unwrap_or_else(|| doze_desk::settings::Cli::go(PathBuf::from("doze")));
        NoRoot(SystemHands::new(cli, self.hub.home().to_path_buf()))
    }

    /// Repair: the steps the failing parts need, shown in the run sheet before anything asks for the password; the
    /// sheet runs them, streams what they print and has doctor look again after.
    pub(crate) fn repair(&mut self, cx: &mut Context<Self>) {
        if self.parts.is_empty() {
            cx.emit(SettingsEvent::Say(
                "doze doctor has not answered yet; Repair waits for its report".into(),
                true,
            ));
            self.refresh(cx);
            return;
        }
        let steps = repair::steps(
            &self.hub.stacks(),
            &self.parts,
            self.at.elsewhere.as_deref(),
        );
        cx.emit(SettingsEvent::Run(Job::Repair(steps)));
    }

    /// `doze doctor` itself, its report streamed into the run sheet (no password).
    pub(crate) fn run_doctor(&mut self, cx: &mut Context<Self>) {
        let dir = crate::facts::doctor_dir(&self.hub.stacks());
        cx.emit(SettingsEvent::Run(Job::Doctor(dir)));
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
        // Clean up, a route and the authority go through the run sheet, which shows what it will do from a fresh look
        // before anything asks for the password, and acts on that alone. A single process or data folder (asked
        // already) is cleared at once, by hands that refuse root.
        if id == "all" || id.starts_with("route:") || id.starts_with("ca:") {
            cx.emit(SettingsEvent::Run(Job::Clear(id.to_owned())));
            cx.notify();
            return;
        }
        let mut hands = self.hands();
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
            leftovers::clear(&id, &m, &mut hands)
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

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn the_login_item_names_the_path_that_survives_an_upgrade() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let cellar = dir.path().join("Cellar/1.0/bin");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&cellar).unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(&bin).unwrap_or_else(|e| panic!("{e}"));
        let exe = cellar.join("doze-app");
        std::fs::write(&exe, "").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(&exe, bin.join("doze-app")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            super::stable(&exe, Some(bin.as_os_str())),
            bin.join("doze-app")
        );
        assert_eq!(super::stable(&exe, None), exe);
        let bundled = std::path::Path::new("/Applications/Doze.app/Contents/MacOS/doze-app");
        assert_eq!(super::stable(bundled, Some(bin.as_os_str())), bundled);
    }
}

/// Hands that never ask for the administrator's password: a clear done at once, outside the run sheet, must not raise
/// the system's prompt for something nobody was shown.
struct NoRoot(SystemHands);

impl doze_desk::settings::Hands for NoRoot {
    fn doze(
        &mut self,
        dir: Option<&std::path::Path>,
        args: &[String],
    ) -> Result<(), doze_desk::settings::Fail> {
        self.0.doze(dir, args)
    }
    fn admin(&mut self, _: &str, _: &str) -> Result<(), doze_desk::settings::Fail> {
        Err(doze_desk::settings::Fail::Failed(
            "this needs the administrator's password: use Clean up, which shows what it runs first"
                .into(),
        ))
    }
    fn terminate(&mut self, pid: u32) -> Result<(), doze_desk::settings::Fail> {
        self.0.terminate(pid)
    }
    fn kill(&mut self, pid: u32) -> Result<(), doze_desk::settings::Fail> {
        self.0.kill(pid)
    }
    fn alive(&mut self, pid: u32) -> bool {
        self.0.alive(pid)
    }
    fn pause(&mut self) {
        self.0.pause();
    }
    fn remove_dir(&mut self, dir: &std::path::Path) -> Result<(), doze_desk::settings::Fail> {
        self.0.remove_dir(dir)
    }
}
