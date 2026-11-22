//! A window glanced at (on screen, not in use): the batches it takes in, at most once a second for a change of state
//! and otherwise on the [`SLOT`] grid; its words to the minute, read again on that grid too; and one wake for when its
//! daemon's last frame would turn old.
//!
//! A change of state is what a person must see now: a stack, or a service, that came up, went down, fell asleep,
//! failed or recovered ([`signature`]). Everything else a live stack does all the time (meters, connection counts,
//! log lines, a service's holders, its journal) and every word that moves with time ("up 9m", "sleeps in 3m") is
//! taken in on a grid of [`SLOT`] (30 s), so a window left alone on a live stack draws at most two frames a minute
//! for them, all of them together. Without the grid a real daemon's instance frames (every three seconds) and each
//! row's "up Nm" turning at its own second drew a frame every one to three seconds, which kept the GPU driver's
//! ~150 MB for good (docs/port/weight.md, "A live stack's idle window").

use std::hash::{Hash as _, Hasher as _};

use std::time::{Duration, Instant, SystemTime};

use doze_desk::{Batch, Parts};
use doze_display as words;
use gpui::{AsyncApp, Context, WeakEntity};

use super::super::inside::Inside;
use super::super::{Root, attention};
use attention::Mode;

/// Glanced at: a change of state is taken in at most this often.
pub(crate) const STATE_EVERY: Duration = Duration::from_secs(1);
/// Glanced at: what is not a change of state (the meters, connections, logs, the words that move with time) is
/// taken in on a grid of this step, all of it at once.
pub(crate) const SLOT: Duration = Duration::from_secs(30);
/// A wake for a word's change lands this long after it, so the word has changed when it is read.
const PAST: Duration = Duration::from_millis(50);

/// The parts that may carry a change of state ([`signature`] says whether they did).
const STATE: Parts = Parts::LIST
    .union(Parts::META)
    .union(Parts::CONN)
    .union(Parts::INSTANCES);

/// What a person must see at once, hashed: each stack, running or not, its link, and each service's state (up,
/// asleep, starting, crashing, failed), error, restarts and switches. Not the meters, connections, holders (held or
/// dozing), logs or times: those move all the time on a live stack, and wait for the grid.
pub(crate) fn signature(stacks: &[doze_desk::StackView]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    for st in stacks {
        st.key.dir().hash(&mut h);
        st.running.hash(&mut h);
        format!("{:?}", st.link.conn).hash(&mut h);
        for v in st.words().instances {
            v.name.hash(&mut h);
            // Held (active) and dozing (idle) are both up: a live stack's connections flip a service between them
            // every second, so the flip waits for the grid; up, asleep, starting, crashing or failed is drawn at once.
            let state = match v.state {
                words::State::Active | words::State::Idle => "up".to_owned(),
                other => format!("{other:?}"),
            };
            (state, format!("{:?}", v.kind)).hash(&mut h);
            v.last_error.hash(&mut h);
            v.restart_count.hash(&mut h);
            (v.paused, v.keep_awake, v.disabled, v.tainted).hash(&mut h);
        }
    }
    h.finish()
}

impl Root {
    /// A batch from the hub, as the window is looked at: in use, taken in now; glanced at, a change of state within a
    /// second and the rest at the next step of the grid; hidden, kept for when the window shows.
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
                let state = !p.hub.is_empty()
                    || (p.stacks.values().any(|parts| parts.intersects(STATE))
                        && Some(signature(&self.hub.stacks())) != self.clock.sig);
                if !state {
                    let at = self.slot_after(now);
                    self.arm_flush(at, cx);
                    return;
                }
                match self.clock.took_state.map(|t| t + STATE_EVERY) {
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
        self.clock.took_state = Some(cx.background_executor().now());
        self.clock.sig = Some(signature(&self.hub.stacks()));
        let ((), next) = words::coarse(|| self.apply_now(&b, cx));
        self.arm_next(next, cx);
    }

    /// The first step of the grid at or after `at`. The grid starts with the first look at a glanced window.
    fn slot_after(&mut self, at: Instant) -> Instant {
        let origin = *self.clock.origin.get_or_insert(at);
        let since = at.saturating_duration_since(origin).as_nanos();
        let steps = since.div_ceil(SLOT.as_nanos());
        origin + SLOT * u32::try_from(steps).unwrap_or(u32::MAX)
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

    /// Glanced at: wakes once, at the step of the grid on or after the first word read would read differently (`in`,
    /// from `coarse`), unless a wake lands sooner already.
    pub(crate) fn arm_next(&mut self, after: Option<Duration>, cx: &mut Context<Self>) {
        let Some(after) = after else {
            return;
        };
        // On the grid: every word that turns within a step is read again once, together, with the meters.
        let now = cx.background_executor().now();
        let at = self.slot_after(now + after + PAST);
        if self.clock.next.as_ref().is_some_and(|(t, _)| *t <= at) {
            return;
        }
        let wait = at.saturating_duration_since(now);
        let task = cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            cx.background_executor().timer(wait).await;
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
        self.clock.sig = Some(signature(&self.hub.stacks()));
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
