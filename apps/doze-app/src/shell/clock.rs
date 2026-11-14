//! The window's clock. A page re-reads the hub when a batch arrives; but a countdown (a dozing row's "sleeps in 3:10",
//! a crashing one's "next in 4s") and a daemon gone quiet ("last update 5s ago") change with time alone. How often
//! they are read again follows how the window is looked at (`attention`), because every frame keeps the GPU driver's
//! ~150 MB (docs/port/weight.md, "the idle window"):
//! - **in use**: once a second, to the second, and every batch as it comes;
//! - **glanced at**: the words are written to the minute (`doze_display::coarse`: "sleeps in 4m", "<1m ago"), and the
//!   page wakes once, when the first of them would read differently, rather than every second. A change of state is
//!   taken in at most once a second, the meters at most every ten; and when the daemon's last frame would turn old
//!   (`doze_display::STALE_AFTER`), one wake says so;
//! - **hidden**: nothing is read or drawn; what came meanwhile is taken in when the window shows again.
//!
//! A page re-renders only when what it shows changed (the views are `PartialEq`), so a wake whose words read the same
//! draws nothing.

use std::time::{Duration, Instant, SystemTime};

use doze_desk::{Batch, StackView};
use doze_display as words;
use gpui::{
    AsyncApp, Context, DispatchPhase, IntoElement, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, ScrollEvent, Styled as _, Task, WeakEntity, Window,
    prelude::FluentBuilder as _,
};

use super::{Root, Screen, attention};
use attention::Mode;

mod glance;

/// The clock's state.
#[derive(Default)]
pub(crate) struct Clock {
    /// In use: a read is due in a second.
    wound: bool,
    /// Wakes when the window stops being in use (see [`Root::sync_hub`]).
    lapse: Option<Task<()>>,
    /// Glanced at or hidden: the batches not taken in yet, merged.
    pending: Option<Batch>,
    /// Glanced at: when a change of state was last taken in, and the meters.
    took_state: Option<Instant>,
    took_meters: Option<Instant>,
    /// Glanced at: the wake that takes `pending` in, and when it lands.
    flush: Option<(Instant, Task<()>)>,
    /// Glanced at: the wake for the first word to read differently, and when it lands.
    next: Option<(Instant, Task<()>)>,
    /// Glanced at: the wake for when the daemon's last frame turns old.
    stale: Option<Task<()>>,
}

impl std::fmt::Debug for Clock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Clock")
            .field("wound", &self.wound)
            .field("pending", &self.pending.is_some())
            .field("flush", &self.flush.as_ref().map(|f| f.0))
            .field("next", &self.next.as_ref().map(|n| n.0))
            .finish_non_exhaustive()
    }
}

impl Clock {
    /// Drops the wakes that only a window glanced at keeps.
    fn calm(&mut self) {
        self.flush = None;
        self.next = None;
        self.stale = None;
    }
}

impl Root {
    /// The window's way of being looked at may have changed: brings the page in line with it.
    fn settle(&mut self, cx: &mut Context<Self>) {
        match attention::mode(cx.background_executor().now()) {
            Mode::Hidden => self.clock.calm(),
            Mode::Glance => {
                self.clock.took_state = None;
                self.clock.took_meters = None;
                self.glance(cx);
            }
            Mode::Use => {
                self.clock.calm();
                if let Some(b) = self.clock.pending.take() {
                    self.apply_now(&b, cx);
                }
                if let Some(a) = self.pages.activity.clone() {
                    a.update(cx, |a, cx| a.read(cx));
                }
                self.reread(cx);
                self.wind(cx);
            }
        }
    }

    /// Runs `f` (a read of what is shown) with its words as the window is looked at: glanced at, to the minute, with a
    /// wake for when the first of them would change. Inside a read that does so already, `f` is part of it.
    pub(crate) fn timed<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>) -> R,
    ) -> R {
        if words::is_coarse() || attention::mode(cx.background_executor().now()) != Mode::Glance {
            return f(self, cx);
        }
        let (out, next) = words::coarse(|| f(self, cx));
        self.arm_next(next, cx);
        out
    }

    /// A page was just shown: its words, and the wakes that keep them true, as the window is looked at.
    pub(crate) fn keep_time(&mut self, cx: &mut Context<Self>) {
        match attention::mode(cx.background_executor().now()) {
            Mode::Use => self.wind(cx),
            Mode::Glance => self.glance(cx),
            Mode::Hidden => {}
        }
    }

    /// Winds the clock when what is shown counts and the window is in use, unless it is wound already.
    pub(crate) fn wind(&mut self, cx: &mut Context<Self>) {
        if self.clock.wound
            || !attention::in_use(cx.background_executor().now())
            || !(self.page_counts() || self.needs_count())
        {
            return;
        }
        self.clock.wound = true;
        cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |r, cx| {
                r.clock.wound = false;
                if !attention::in_use(cx.background_executor().now()) {
                    return;
                }
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

    /// Follows how the window is looked at: it becoming key (or not), shown or hidden, the person's keys; the pointer
    /// is `touches`'s.
    pub(super) fn follow_use(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        attention::reset();
        attention::set_visible(window.is_visible());
        attention::key(window.is_window_active(), cx.background_executor().now());
        cx.observe_window_activation(window, |this, window, cx| {
            if attention::key(window.is_window_active(), cx.background_executor().now()) {
                this.came_back(cx);
            } else {
                this.sync_hub(cx);
                this.settle(cx);
            }
        })
        .detach();
        // Minimised, covered, on another Space or a sleeping display: nothing drawn, no meters (Go's
        // visibilitychange blur); shown again, what changed meanwhile is read at once.
        cx.observe_window_visibility(window, |this, v, _, cx| {
            if attention::visible() == v.is_visible() {
                return;
            }
            attention::set_visible(v.is_visible());
            this.sync_hub(cx);
            this.settle(cx);
        })
        .detach();
        self.sync_hub(cx);
        cx.observe_keystrokes(|this, _, _, cx| this.touched(cx))
            .detach();
    }

    /// The person touched the window.
    pub(super) fn touched(&mut self, cx: &mut Context<Self>) {
        if attention::touch(cx.background_executor().now()) {
            self.came_back(cx);
        }
    }

    /// Back in use after a time untouched: what is shown is read again now, to the second, and the clocks run again.
    fn came_back(&mut self, cx: &mut Context<Self>) {
        self.sync_hub(cx);
        self.settle(cx);
    }

    /// The meters stream while the window is on screen (glanced at, the page takes them in every ten seconds), not
    /// while it is hidden. A hands-off run leaves the hub as its harness set it. While the window is in use, one wake
    /// for the moment it stops being in use, to write its words to the minute from then on.
    pub(crate) fn sync_hub(&mut self, cx: &mut Context<Self>) {
        if !crate::handsoff::on() {
            self.hub.set_visible(attention::wants_meters());
        }
        let now = cx.background_executor().now();
        let Some(wait) = attention::lapses_in(now) else {
            self.clock.lapse = None;
            return;
        };
        if self.clock.lapse.is_some() {
            return;
        }
        self.clock.lapse = Some(cx.spawn(
            async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |r, cx| {
                    r.clock.lapse = None;
                    r.sync_hub(cx);
                    if !attention::attended(cx.background_executor().now()) {
                        r.settle(cx);
                    }
                });
            },
        ));
    }

    /// Hears the pointer (moves, presses, the wheel) in the capture phase, as touches; it stops nothing. Behind the
    /// hands-off wall, when a run has one, so a person's pointer is neither acted on nor counted as a touch.
    pub(super) fn touches(&self, cx: &Context<Self>) -> impl IntoElement {
        let me = cx.weak_entity();
        let heard = gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                heard::<MouseMoveEvent>(window, me.clone());
                heard::<MouseDownEvent>(window, me.clone());
                heard::<ScrollEvent>(window, me.clone());
            },
        );
        gpui::div()
            .absolute()
            .size_0()
            .when(crate::handsoff::on(), |d| {
                d.child(crate::handsoff::pointer_wall())
            })
            .child(heard)
    }

    /// The stack page shows a countdown, or a metered stack whose daemon may have gone quiet.
    fn page_counts(&self) -> bool {
        let Screen::Stack(k) = &self.screen else {
            return false;
        };
        self.hub
            .stack(k)
            .is_some_and(|st| st.running && (counts(&st, self.hub.now()) || st.metered))
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

fn heard<E: gpui::MouseEvent>(window: &mut Window, me: WeakEntity<Root>) {
    window.on_mouse_event(move |_: &E, phase, _, cx| {
        if phase == DispatchPhase::Capture {
            let _ = me.update(cx, |r, cx| r.touched(cx));
        }
    });
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
