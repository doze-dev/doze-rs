//! Version skew as the stack page needs it (doze-desk `skew`): whether the stack's daemon, or the `doze` the app runs,
//! is older than the doze the app carries. Asked off the UI thread (it runs `doze --json version`) and kept for as
//! long as the daemon's pid is the same: a program does not change under a running process.

use std::path::PathBuf;

use doze_desk::StackView;
use doze_desk::skew::{self, DozeVersion, Skew, Whose};
use gpui::{AsyncApp, Context, WeakEntity};

use super::{Facts, Mode};

/// The doze this app carries.
pub(crate) fn own() -> DozeVersion {
    DozeVersion {
        version: doze_cli::ctx::VERSION.to_owned(),
        protocol: doze_module_proto::version::CURRENT.major,
        commit: String::new(),
    }
}

/// What a check found: the program it looked at and the skew, if any.
pub(super) type Found = Option<Skew>;

impl Facts {
    /// The skew banner for `st`: the daemon's program when it is older than the app, else the `doze` the app runs
    /// when that is. `None` in a fixture, and while the first answer is coming.
    pub(crate) fn skew(&mut self, st: &StackView, cx: &mut Context<Self>) -> Option<Skew> {
        let Mode::Live(cli) = &self.mode else {
            return None;
        };
        let cli = cli.clone();
        let daemon = st
            .running
            .then(|| doze_projects::daemon_pid(&st.key))
            .flatten()
            .filter(|p| p.alive)
            .map(|p| p.pid);
        if let Some(pid) = daemon {
            if let Some(found) = self.skews.get(&pid) {
                if found.is_some() {
                    return found.clone();
                }
            } else {
                self.ask_skew(Some(pid), None, cx);
            }
        }
        match &self.cli_skew {
            Some(found) => found.clone(),
            None => {
                if let Some(program) = cli {
                    self.ask_skew(None, Some(program), cx);
                }
                None
            }
        }
    }

    /// Asks, off the UI thread, what the daemon `pid` runs or what `program` is.
    fn ask_skew(&mut self, pid: Option<u32>, program: Option<PathBuf>, cx: &mut Context<Self>) {
        let key = pid.unwrap_or(0);
        if !self.skew_asked.insert(key) {
            return;
        }
        let job = cx.background_executor().spawn(async move {
            let me = std::env::current_exe().ok();
            let (whose, program) = match (pid, program) {
                (Some(pid), _) => (Whose::Daemon, skew::program_of(pid)?),
                (None, Some(p)) => (Whose::Cli, p),
                (None, None) => return None,
            };
            if me.is_some_and(|m| skew::same_program(&m, &program)) {
                return None;
            }
            skew::skew(whose, &program, skew::ask(&program).as_ref(), &own())
        });
        cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
            let found: Found = job.await;
            let _ = this.update(cx, |f, cx| {
                f.skew_asked.remove(&key);
                let changed = found.is_some();
                match pid {
                    Some(pid) => {
                        f.skews.insert(pid, found);
                    }
                    None => f.cli_skew = Some(found),
                }
                if changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
