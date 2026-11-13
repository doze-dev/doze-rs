//! The window's clock. A page re-reads the hub when a batch arrives, and a running daemon sends the meters every second
//! while a page watches them; but a countdown (a dozing row's "sleeps in 3:10", a crashing one's "next in 4s") and a
//! daemon gone quiet ("last update 5s ago") change with time alone. So while what is shown has one of them, the
//! window reads it again once a second, and only then: an idle window with nothing counting stays at zero redraws.

use std::time::{Duration, SystemTime};

use doze_desk::StackView;
use doze_display as words;
use gpui::{AsyncApp, Context, WeakEntity};

use super::{Root, Screen};

/// Whether the clock is wound: a read is due in a second.
#[derive(Debug, Default)]
pub(crate) struct Clock {
    wound: bool,
}

impl Root {
    /// Winds the clock when what is shown counts, unless it is wound already.
    pub(crate) fn wind(&mut self, cx: &mut Context<Self>) {
        if self.clock.wound || !(self.page_counts() || self.needs_count()) {
            return;
        }
        self.clock.wound = true;
        cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |r, cx| {
                r.clock.wound = false;
                if r.page_counts() {
                    r.refresh(cx);
                }
                if r.needs_count() {
                    r.rebuild_attention(cx);
                }
                r.wind(cx);
            });
        })
        .detach();
    }

    /// The stack page shows a countdown, or a metered stack whose daemon may have gone quiet.
    fn page_counts(&self) -> bool {
        let Screen::Stack(k) = &self.screen else {
            return false;
        };
        self.hub.stack(k).is_some_and(|st| {
            st.running && (counts(&st, self.hub.now()) || (st.metered && serving(&st)))
        })
    }

    /// Needs you says "next in 4s" of a crashing instance somewhere.
    fn needs_count(&self) -> bool {
        let now = self.hub.now();
        self.hub.stacks().iter().any(|st| {
            st.running
                && st
                    .words()
                    .instances
                    .iter()
                    .any(|v| next_try(v, now).is_some())
        })
    }
}

/// When a crashing instance tries again, while that is still ahead.
fn next_try(v: &words::Instance, now: SystemTime) -> Option<SystemTime> {
    (v.state == words::State::Crashing)
        .then_some(v.next_restart)
        .flatten()
        .filter(|t| *t > now)
}

/// Something on the stack counts down: a dozing instance to its sleep, or a crashing one to its next try.
fn counts(st: &StackView, now: SystemTime) -> bool {
    st.words().instances.iter().any(|v| {
        next_try(v, now).is_some()
            || (v.state == words::State::Idle && v.idle_since.is_some() && !v.always_on)
    })
}

/// Something on it is up, so its meters are due every second.
fn serving(st: &StackView) -> bool {
    st.words().instances.iter().any(words::awake_now)
}
