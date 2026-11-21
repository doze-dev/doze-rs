//! A window glanced at (on screen, not in use): the batches it takes in, at most once a second for a change of state
//! and every ten for the meters; its words to the minute, read again once, when the first of them would change; and
//! one wake for when its daemon's last frame would turn old.

use std::time::{Duration, Instant, SystemTime};

use doze_desk::{Batch, Parts};
use doze_display as words;
use gpui::{AsyncApp, Context, WeakEntity};

use super::super::inside::Inside;
use super::super::{Root, attention};
use attention::Mode;

/// Glanced at: a change of state is taken in at most this often.
pub(crate) const STATE_EVERY: Duration = Duration::from_secs(1);
/// Glanced at: the meters are taken in at most this often.
pub(crate) const METERS_EVERY: Duration = Duration::from_secs(10);
/// A wake for a word's change lands this long after it, so the word has changed when it is read.
const PAST: Duration = Duration::from_millis(50);

/// Every part of a stack but the meters: a change of state.
const STATE: Parts = Parts::LIST
    .union(Parts::META)
    .union(Parts::CONN)
    .union(Parts::INSTANCES)
    .union(Parts::JOURNAL)
    .union(Parts::LOGS)
    .union(Parts::CONSOLE);

impl Root {
    /// A batch from the hub, as the window is looked at: in use, taken in now; glanced at, a change of state within a
    /// second and the meters within ten; hidden, kept for when the window shows.
    pub(crate) fn apply(&mut self, batch: &Batch, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match attention::mode(now) {
            Mode::Use => self.apply_now(batch, cx),
            Mode::Hidden => self.keep(batch),
            Mode::Glance => {
                self.keep(batch);
                self.arm_stale(cx);
                let Some(p) = &self.clock.pending else {
                    return;
                };
                let state =
                    !p.hub.is_empty() || p.stacks.values().any(|parts| parts.intersects(STATE));
                let due = if state {
                    self.clock.took_state.map(|t| t + STATE_EVERY)
                } else {
                    self.clock.took_meters.map(|t| t + METERS_EVERY)
                };
                match due {
                    Some(at) if at > now => self.arm_flush(at, cx),
                    _ => self.flush(cx),
                }
            }
        }
    }

    fn keep(&mut self, batch: &Batch) {
        self.clock
            .pending
            .get_or_insert_with(Batch::default)
            .merge(batch.clone());
    }

    /// Glanced at: takes in what was kept, in words to the minute.
    fn flush(&mut self, cx: &mut Context<Self>) {
        self.clock.flush = None;
        let Some(b) = self.clock.pending.take() else {
            return;
        };
        let now = cx.background_executor().now();
        self.clock.took_state = Some(now);
        if b.stacks.values().any(|p| p.intersects(Parts::STATS)) {
            self.clock.took_meters = Some(now);
        }
        let ((), next) = words::coarse(|| self.apply_now(&b, cx));
        self.arm_next(next, cx);
    }

    fn arm_flush(&mut self, at: Instant, cx: &mut Context<Self>) {
        if self.clock.flush.as_ref().is_some_and(|(t, _)| *t <= at) {
            return;
        }
        let wait = at.saturating_duration_since(cx.background_executor().now());
        let task = cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |r, cx| {
                r.clock.flush = None;
                if attention::mode(cx.background_executor().now()) == Mode::Glance {
                    r.flush(cx);
                }
            });
        });
        self.clock.flush = Some((at, task));
    }

    /// Glanced at: wakes once when the first word read would read differently (`in`, from `coarse`), unless a wake
    /// lands sooner already.
    pub(crate) fn arm_next(&mut self, after: Option<Duration>, cx: &mut Context<Self>) {
        let Some(after) = after else {
            return;
        };
        let at = cx.background_executor().now() + after + PAST;
        if self.clock.next.as_ref().is_some_and(|(t, _)| *t <= at) {
            return;
        }
        let task = cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            cx.background_executor().timer(after + PAST).await;
            let _ = this.update(cx, |r, cx| {
                r.clock.next = None;
                if attention::mode(cx.background_executor().now()) == Mode::Glance {
                    r.glance(cx);
                }
            });
        });
        self.clock.next = Some((at, task));
    }

    /// Glanced at: one wake for when the shown stack's daemon, metered and quiet since its last frame, would be old
    /// (`doze_display::STALE_AFTER`), so a frozen daemon's banner shows without the page being read every second. A
    /// frame since (a batch, or `last_frame` moved) puts the wake off.
    fn arm_stale(&mut self, cx: &mut Context<Self>) {
        self.clock.stale = None;
        let now = self.hub.now();
        let Some(at) = self.quiet_at() else {
            return;
        };
        let Ok(wait) = at.duration_since(now) else {
            return; // old already: the page said so when it was read
        };
        self.clock.stale = Some(cx.spawn(
            async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
                cx.background_executor().timer(wait + PAST).await;
                let _ = this.update(cx, |r, cx| {
                    r.clock.stale = None;
                    if attention::mode(cx.background_executor().now()) != Mode::Glance {
                        return;
                    }
                    let quiet = r.quiet_at().is_some_and(|at| at <= r.hub.now());
                    if quiet {
                        r.glance(cx);
                    } else {
                        r.arm_stale(cx);
                    }
                });
            },
        ));
    }

    /// When the shown stack's picture turns old if no frame comes: a running, metered stack, from its last frame.
    fn quiet_at(&self) -> Option<SystemTime> {
        let st = self.hub.stack(self.key()?)?;
        if !st.running || !st.metered {
            return None;
        }
        st.link.last_frame.map(|t| t + words::STALE_AFTER)
    }

    /// Glanced at: everything shown read again, in words to the minute, and the next wake set.
    pub(super) fn glance(&mut self, cx: &mut Context<Self>) {
        self.clock.next = None;
        let pending = self.clock.pending.take();
        let ((), next) = words::coarse(|| {
            if let Some(b) = &pending {
                self.apply_now(b, cx);
            }
            self.reread(cx);
        });
        self.arm_next(next, cx);
        self.arm_stale(cx);
    }

    /// Everything shown read again: the page, Needs you, the page inside a stack.
    pub(super) fn reread(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.rebuild_attention(cx);
        if let Some(Inside::Instance(v)) = &self.inside {
            v.update(cx, |v, cx| v.refresh(cx));
        }
    }
}
