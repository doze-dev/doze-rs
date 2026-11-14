//! The window's clock. A page re-reads the hub when a batch arrives, and a running daemon sends the meters every second
//! while a page watches them; but a countdown (a dozing row's "sleeps in 3:10", a crashing one's "next in 4s") and a
//! daemon gone quiet ("last update 5s ago") change with time alone. So while what is shown has one of them, and the
//! person is using the window (`attention`), the window reads it again once a second, and only then: an untouched
//! window draws no frames of its own (each would keep the GPU driver's ~150 MB), and a touch reads it all again at once.

use std::time::{Duration, SystemTime};

use doze_desk::StackView;
use doze_display as words;
use gpui::{
    AsyncApp, Context, DispatchPhase, IntoElement, MouseDownEvent, MouseMoveEvent,
    ParentElement as _, ScrollEvent, Styled as _, WeakEntity, Window, prelude::FluentBuilder as _,
};

use super::inside::Inside;
use super::{Root, Screen, attention};

/// Whether the clock is wound: a read is due in a second.
#[derive(Debug, Default)]
pub(crate) struct Clock {
    wound: bool,
    /// Wakes when the window stops being in use, to stop the meters (see [`Root::sync_hub`]).
    lapse: Option<gpui::Task<()>>,
}

impl Root {
    /// Winds the clock when what is shown counts and the window is in use, unless it is wound already.
    pub(crate) fn wind(&mut self, cx: &mut Context<Self>) {
        if self.clock.wound
            || !attention::attended(cx.background_executor().now())
            || !(self.page_counts() || self.needs_count())
        {
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

    /// Follows the person's use of the window: it becoming key (or not), their keys; the pointer is `touches`'s.
    pub(super) fn follow_use(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        attention::reset();
        attention::key(window.is_window_active(), cx.background_executor().now());
        cx.observe_window_activation(window, |this, window, cx| {
            if attention::key(window.is_window_active(), cx.background_executor().now()) {
                this.came_back(cx);
            } else {
                this.sync_hub(cx);
            }
        })
        .detach();
        // Minimised, behind another window or on a sleeping display: no meters (Go's visibilitychange blur).
        cx.observe_window_visibility(window, |this, v, _, cx| {
            attention::set_visible(v.is_visible());
            this.sync_hub(cx);
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

    /// Back in use after a time untouched: what is shown is read again now, and the clocks run again.
    fn came_back(&mut self, cx: &mut Context<Self>) {
        self.sync_hub(cx);
        if let Some(a) = self.pages.activity.clone() {
            a.update(cx, |a, cx| a.read(cx));
        }
        self.refresh(cx);
        self.rebuild_attention(cx);
        self.wind(cx);
        if let Some(Inside::Instance(v)) = &self.inside {
            v.update(cx, |v, cx| v.refresh(cx));
        }
    }

    /// The meters stream only while the window is on screen and in use: from a real daemon they come every second, and
    /// a figure that changed is a frame, which would keep the GPU's ~150 MB (`attention`). Untouched, the page keeps the
    /// last figures and still hears every change of state (at most once a second); a touch brings the meters back. A
    /// hands-off run leaves the hub as its harness set it.
    pub(crate) fn sync_hub(&mut self, cx: &mut Context<Self>) {
        if crate::handsoff::on() {
            return;
        }
        let now = cx.background_executor().now();
        self.hub.set_visible(attention::wants_meters(now));
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
