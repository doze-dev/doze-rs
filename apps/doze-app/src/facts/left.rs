//! What stacks left behind, counted every 15 s off the UI thread (Go `leftoversWithin`), for Needs you. Only counted:
//! nothing here stops or deletes anything. A fixture's stacks are made up, so this machine's processes are not theirs
//! to judge: a fixture counts none.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use doze_desk::StackView;
use doze_desk::leftovers::{self, Counts};
use gpui::{AsyncApp, Context, WeakEntity};

use super::{Facts, Mode};

/// How often the leftovers are counted again (Go `leftoverEvery`).
const LEFTOVER_EVERY: Duration = Duration::from_secs(15);

impl Facts {
    /// The leftovers as last counted; counts again when that is old.
    pub(crate) fn leftovers(
        &mut self,
        home: &Path,
        stacks: &[StackView],
        cx: &mut Context<Self>,
    ) -> Counts {
        if matches!(self.mode, Mode::Fixture) {
            return Counts::default();
        }
        let fresh = self
            .left
            .is_some_and(|(at, _)| at.elapsed() < LEFTOVER_EVERY);
        if !fresh && !self.left_asked {
            self.left_asked = true;
            let home = PathBuf::from(home);
            let files: Vec<PathBuf> = stacks
                .iter()
                .filter(|s| !s.dir.is_empty())
                .map(|s| Path::new(&s.dir).join("doze.hcl"))
                .collect();
            let job = cx
                .background_executor()
                .spawn(async move { leftovers::count(&home, &files) });
            cx.spawn(async move |this: WeakEntity<Facts>, cx: &mut AsyncApp| {
                let got = job.await;
                let _ = this.update(cx, |f, cx| {
                    f.left_asked = false;
                    let changed = f.left.is_none_or(|(_, c)| c != got);
                    f.left = Some((Instant::now(), got));
                    if changed {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        self.left.map(|(_, c)| c).unwrap_or_default()
    }
}
