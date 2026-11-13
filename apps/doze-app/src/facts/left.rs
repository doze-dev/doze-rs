//! What stacks left behind (Go `serve/leftovers.go`): the one look at the machine, shared by Needs you (its counts,
//! every 15 s while the window is open, Go `leftoversWithin`) and Settings › Machine (the list, each with its way to
//! clear it). The look is `doze_desk::settings::leftovers::{gather, find}`, run off the UI thread, and only reads:
//! `ps`, the home's folders, the routes record, the keychain's word on the authority. Nothing here stops or deletes
//! anything; clearing is Settings', one click at a time.
//!
//! A fixture's stacks are made up, so this machine's processes are not theirs to judge: a fixture finds none (its
//! Settings shows a staged machine instead).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use doze_desk::StackView;
use doze_desk::settings::Leftover;
use doze_desk::settings::leftovers;
use gpui::{AsyncApp, Context, WeakEntity};

use super::{Facts, Mode};

/// How often the machine is looked at again (Go `leftoverEvery`).
const LEFTOVER_EVERY: Duration = Duration::from_secs(15);

impl Facts {
    /// What stacks left behind, as last found (`None` until the first look ends); looks again when that is old.
    pub(crate) fn leftovers(
        &mut self,
        home: &Path,
        stacks: &[StackView],
        cx: &mut Context<Self>,
    ) -> Option<Vec<Leftover>> {
        if matches!(self.mode, Mode::Fixture) {
            return Some(Vec::new());
        }
        let fresh = self
            .left
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < LEFTOVER_EVERY);
        if !fresh && !self.left_asked {
            self.left_asked = true;
            let home = PathBuf::from(home);
            let stacks = stacks.to_vec();
            let admin = doze_os::privilege::admin_supported();
            let job = cx
                .background_executor()
                .spawn(async move { leftovers::find(&leftovers::gather(&home, &stacks, admin)) });
            cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
                let got = job.await;
                let _ = this.update(cx, |f, cx| {
                    f.left_asked = false;
                    let changed = f.left.as_ref().is_none_or(|(_, l)| *l != got);
                    f.left = Some((Instant::now(), got));
                    // A look Settings asked for ends its "looking…" even when nothing changed.
                    let wanted = std::mem::take(&mut f.left_wanted);
                    if changed || wanted {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        self.left.as_ref().map(|(_, l)| l.clone())
    }

    /// A look is under way.
    pub(crate) fn looking_left(&self) -> bool {
        self.left_asked
    }

    /// Makes the last look old, so the next asks again (after a clear); what it found stays until the new answer.
    pub(crate) fn forget_leftovers(&mut self) {
        self.left_wanted = true;
        if let Some((at, _)) = &mut self.left {
            *at = at.checked_sub(LEFTOVER_EVERY).unwrap_or(*at);
        }
    }
}
