//! The run sheet's place in the window: a privileged job (Repair, a clean-up that needs the password, doze doctor) opens
//! over the page it was asked from, and stays alive after its sheet is hidden, so a job still running says how it
//! ended.

use doze_desk::settings::Job;
use gpui::{AppContext as _, Context, Window};

use super::Root;
use crate::overlays::Overlay;
use crate::overlays::admin::{AdminEvent, AdminView};

impl Root {
    /// Shows `job` in the run sheet; nothing runs until its Run is pressed.
    pub(crate) fn open_admin(&mut self, job: Job, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(a) = &self.pages.admin
            && matches!(
                a.read(cx).phase,
                crate::overlays::admin::Phase::Running | crate::overlays::admin::Phase::Checking
            )
        {
            // One privileged job at a time: the one running comes back into view.
            let a = a.clone();
            self.show_overlay(Overlay::Admin(a), cx);
            return;
        }
        let repair = matches!(job, Job::Repair(_));
        let deps = (
            self.hub.clone(),
            self.pages.facts.clone(),
            self.pages.fixture,
        );
        let v = cx.new(|cx| AdminView::new(job, deps, window, cx));
        cx.subscribe_in(
            &v,
            window,
            move |this, v, ev: &AdminEvent, window, cx| match ev {
                AdminEvent::Close => {
                    if matches!(&this.overlay, Some(Overlay::Admin(o)) if o == v) {
                        this.shut(window, cx);
                    }
                }
                AdminEvent::Busy(b) => {
                    let b = *b;
                    if repair && let Some(s) = this.pages.settings.clone() {
                        s.update(cx, |s, cx| {
                            s.repairing = b;
                            cx.notify();
                        });
                    }
                    // Home's machine line says "Setting up…" while a Repair runs, as Go's Library did.
                    if repair {
                        this.library.update(cx, |l, cx| {
                            l.repairing = b;
                            cx.notify();
                        });
                    }
                }
                AdminEvent::Say(said, bad) => {
                    let (said, bad) = (said.clone(), *bad);
                    this.acts.update(cx, |a, cx| a.say(said, bad, cx));
                    if let Some(s) = this.pages.settings.clone() {
                        s.update(cx, |s, cx| s.look(cx));
                    }
                }
            },
        )
        .detach();
        self.pages.admin = Some(v.clone());
        self.show_overlay(Overlay::Admin(v), cx);
    }
}
