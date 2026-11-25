//! The table of fault rules: bounded, in memory, and the one place a rule is made, changed, fired or ended.
//!
//! A request costs one relaxed load while no rule is switched on ([`Faults::armed`]). With rules, the table is locked for the few
//! comparisons a request needs; it never holds the lock across an await, and nothing grows without a bound: 50 rules, 200 ended
//! ones, eight access keys a rule. Expiry is lazy (no timer anywhere): every look at the table first ends the rules whose time is
//! up, dated when they were due.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use aws_gateway::{Ask, Faults, Tag, Verdict};

use super::codes;
use super::record::{Ended, EndedWhy, First, Found, Rule};
use super::rule::{AddError, Effect, MOST_ENDED, MOST_KEPT_CALLERS, MOST_RULES, Share, Spec};

/// What the table asks the time of: a test makes its own.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Milliseconds on a clock that only goes forward.
    fn mono_ms(&self) -> u64;
    /// Milliseconds since the epoch, for showing.
    fn wall_ms(&self) -> i64;
}

/// The system's clocks.
#[derive(Debug)]
pub struct SystemClock(std::time::Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Clock for SystemClock {
    fn mono_ms(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn wall_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
}

/// A rule in force and the rest of what the table keeps for it.
#[derive(Debug)]
struct Live {
    rule: Rule,
    ends_mono: u64,
    /// Calls that fitted since it last fired (for `Share::Nth`).
    since: u32,
}

#[derive(Debug)]
struct Inner {
    rules: Vec<Live>,
    ended: VecDeque<Ended>,
    next_id: u32,
    rng: u64,
}

impl Inner {
    /// A number from a splitmix64 generator: seeded, so the same seed and the same calls give the same hits.
    fn random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn end(&mut self, index: usize, why: EndedWhy, at_ms: i64) {
        if index >= self.rules.len() {
            return;
        }
        let live = self.rules.remove(index);
        if self.ended.len() == MOST_ENDED {
            self.ended.pop_back();
        }
        self.ended.push_front(Ended {
            rule: live.rule,
            at_ms,
            why,
        });
    }
}

/// The rules.
pub struct LabTable {
    switched_on: AtomicUsize,
    inner: Mutex<Inner>,
    clock: Box<dyn Clock>,
}

impl fmt::Debug for LabTable {
    /// Counts only: nothing a rule matches on is printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LabTable")
            .field("on", &self.switched_on.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl LabTable {
    /// A table with no rules, on the system's clocks.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let clock = SystemClock::default();
        let seed = u64::try_from(clock.wall_ms()).unwrap_or(1);
        Self::with(Box::new(clock), seed)
    }

    /// A table on a clock of the caller's, with a seed for the chances and the jitter (a test's).
    #[must_use]
    pub fn with(clock: Box<dyn Clock>, seed: u64) -> Arc<Self> {
        Arc::new(Self {
            switched_on: AtomicUsize::new(0),
            inner: Mutex::new(Inner {
                rules: Vec::new(),
                ended: VecDeque::new(),
                next_id: 1,
                rng: seed,
            }),
            clock,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn recount(&self, inner: &Inner) {
        let on = inner.rules.iter().filter(|l| l.rule.enabled).count();
        self.switched_on.store(on, Ordering::Relaxed);
    }

    /// Ends every rule whose time is up, dated when it was due.
    fn sweep(&self, inner: &mut Inner, now: u64) {
        let mut changed = false;
        let mut i = 0;
        while i < inner.rules.len() {
            if inner.rules.get(i).is_some_and(|l| l.ends_mono <= now) {
                let at = inner.rules.get(i).map_or(0, |l| l.rule.ends_ms);
                inner.end(i, EndedWhy::Expired, at);
                changed = true;
            } else {
                i += 1;
            }
        }
        if changed {
            self.recount(inner);
        }
    }

    /// Makes a rule. One with the same matcher and the same kind of effect is replaced (and does not count twice against the limit).
    ///
    /// # Errors
    /// The rule is not allowed ([`Spec::check`]), or there are already [`MOST_RULES`].
    pub fn add(&self, spec: Spec) -> Result<u32, AddError> {
        spec.check()?;
        let now = self.clock.mono_ms();
        let wall = self.clock.wall_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let replaced = inner.rules.iter().position(|l| l.rule.spec.same_as(&spec));
        if replaced.is_none() && inner.rules.len() >= MOST_RULES {
            return Err(AddError::Full);
        }
        if let Some(i) = replaced {
            inner.end(i, EndedWhy::Replaced, wall);
        }
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1).max(1);
        let ttl_ms = u64::try_from(spec.ttl.as_millis()).unwrap_or(u64::MAX);
        let name = if spec.name.trim().is_empty() {
            super::words::default_name(&spec)
        } else {
            spec.name.clone()
        };
        let rule = Rule {
            id,
            name: name.into(),
            created_ms: wall,
            ends_ms: wall.saturating_add(i64::try_from(ttl_ms).unwrap_or(i64::MAX)),
            enabled: true,
            matched: 0,
            hits: 0,
            first: None,
            last_ms: None,
            callers: Vec::new(),
            more_callers: 0,
            spec,
        };
        inner.rules.push(Live {
            rule,
            ends_mono: now.saturating_add(ttl_ms),
            since: 0,
        });
        self.recount(&inner);
        Ok(id)
    }

    /// Whether `spec` could be added now: there is room, or it takes the place of a rule.
    #[must_use]
    pub fn has_room_for(&self, spec: &Spec) -> bool {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        inner.rules.len() < MOST_RULES || inner.rules.iter().any(|l| l.rule.spec.same_as(spec))
    }

    /// Ends a rule now. `false`: there is none in force with that number (it may have ended already).
    pub fn remove(&self, id: u32) -> bool {
        let (now, wall) = (self.clock.mono_ms(), self.clock.wall_ms());
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let found = inner.rules.iter().position(|l| l.rule.id == id);
        if let Some(i) = found {
            inner.end(i, EndedWhy::Cleared, wall);
            self.recount(&inner);
        }
        found.is_some()
    }

    /// Ends every rule in force; how many there were.
    pub fn stop_all(&self) -> usize {
        let (now, wall) = (self.clock.mono_ms(), self.clock.wall_ms());
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let n = inner.rules.len();
        while !inner.rules.is_empty() {
            inner.end(0, EndedWhy::Cleared, wall);
        }
        self.recount(&inner);
        n
    }

    /// Switches a rule on or off; it runs out all the same. `false`: no such rule in force.
    pub fn set_enabled(&self, id: u32, on: bool) -> bool {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let found = inner.rules.iter_mut().find(|l| l.rule.id == id);
        let had = found.is_some();
        if let Some(l) = found {
            l.rule.enabled = on;
        }
        self.recount(&inner);
        had
    }

    /// The rules in force, oldest first.
    #[must_use]
    pub fn rules(&self) -> Vec<Rule> {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        inner.rules.iter().map(|l| l.rule.clone()).collect()
    }

    /// The rules that are over, newest first (at most [`MOST_ENDED`]).
    #[must_use]
    pub fn ended(&self) -> Vec<Ended> {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        inner.ended.iter().cloned().collect()
    }

    /// One rule, in force or over (while the history still holds it).
    #[must_use]
    pub fn find(&self, id: u32) -> Option<Found> {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        if let Some(l) = inner.rules.iter().find(|l| l.rule.id == id) {
            return Some(Found {
                rule: l.rule.clone(),
                over: None,
            });
        }
        inner.ended.iter().find(|e| e.rule.id == id).map(|e| Found {
            rule: e.rule.clone(),
            over: Some((e.at_ms, e.why)),
        })
    }

    /// The name of rule `id`, in force or over, while the table still holds it.
    #[must_use]
    pub fn name_of(&self, id: u32) -> Option<Arc<str>> {
        let inner = self.lock();
        inner
            .rules
            .iter()
            .map(|l| &l.rule)
            .chain(inner.ended.iter().map(|e| &e.rule))
            .find(|r| r.id == id)
            .map(|r| Arc::clone(&r.name))
    }

    /// Forgets the rules that are over; how many.
    pub fn clear_ended(&self) -> usize {
        let mut inner = self.lock();
        let n = inner.ended.len();
        inner.ended.clear();
        n
    }

    /// The time, in milliseconds since the epoch, on the table's clock.
    #[must_use]
    pub fn now_ms(&self) -> i64 {
        self.clock.wall_ms()
    }
}

/// How much a firing rule outweighs another: a drop before a fail before a delay, the longer delay of two. For a delay the second
/// number is the wait in milliseconds, jitter drawn.
type Weight = (u8, u64);

/// Whether the share lets this call through.
fn gate(l: &mut Live, draw: u64) -> bool {
    match l.rule.spec.share {
        Share::Every => true,
        Share::Nth(n) => {
            if l.since >= n {
                l.since = 0;
                true
            } else {
                false
            }
        }
        Share::Percent(p) => (draw >> 8) % 100 < u64::from(p),
    }
}

fn weigh(effect: &Effect, draw: u64) -> Weight {
    match effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => {
            let extra = if *jitter_ms == 0 {
                0
            } else {
                (draw >> 16) % (u64::from(*jitter_ms) + 1)
            };
            (1, u64::from(*delay_ms) + extra)
        }
        Effect::Fail(_) => (2, 0),
        Effect::Drop => (3, 0),
    }
}

/// Counts the call against every rule that fits it, and picks the rule that acts.
fn choose(inner: &mut Inner, ask: &Ask<'_>) -> Option<(usize, Weight)> {
    let mut best: Option<(usize, Weight)> = None;
    for i in 0..inner.rules.len() {
        let fits = inner
            .rules
            .get(i)
            .is_some_and(|l| l.rule.enabled && l.rule.spec.matcher.fits(ask));
        if !fits {
            continue;
        }
        let draw = inner.random();
        let Some(l) = inner.rules.get_mut(i) else {
            continue;
        };
        l.rule.matched += 1;
        l.since = l.since.saturating_add(1);
        if !gate(l, draw) {
            continue;
        }
        let w = weigh(&l.rule.spec.effect, draw);
        if best.is_none_or(|(_, b)| w > b) {
            best = Some((i, w));
        }
    }
    best
}

fn clip(text: &str, most: usize) -> Box<str> {
    text.chars().take(most).collect::<String>().into()
}

/// Puts the hit in the rule's record.
fn record(rule: &mut Rule, ask: &Ask<'_>, wall: i64) {
    rule.hits += 1;
    rule.last_ms = Some(wall);
    let caller = clip(ask.access_key_id, 64);
    if rule.first.is_none() {
        rule.first = Some(First {
            at_ms: wall,
            caller: caller.clone(),
            service: ask.sdk_id,
            operation: clip(ask.operation, 64),
            resource: (!ask.resource.is_empty()).then(|| clip(ask.resource, 256)),
            sdk: Some(crate::agent::label(ask.user_agent))
                .filter(|s| !s.is_empty())
                .map(Into::into),
        });
    }
    if !caller.is_empty() && !rule.callers.contains(&caller) {
        if rule.callers.len() < MOST_KEPT_CALLERS {
            rule.callers.push(caller);
        } else {
            rule.more_callers = rule.more_callers.saturating_add(1);
        }
    }
}

impl Faults for LabTable {
    fn armed(&self) -> bool {
        self.switched_on.load(Ordering::Relaxed) > 0
    }

    fn judge(&self, ask: &Ask<'_>) -> Verdict {
        if !self.armed() {
            return Verdict::Pass;
        }
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let Some((index, (_, wait_ms))) = choose(&mut inner, ask) else {
            return Verdict::Pass;
        };
        let wall = self.clock.wall_ms();
        let Some(l) = inner.rules.get_mut(index) else {
            return Verdict::Pass;
        };
        record(&mut l.rule, ask, wall);
        let tag = Tag {
            rule: l.rule.id,
            name: Arc::clone(&l.rule.name),
        };
        let effect = l.rule.spec.effect.clone();
        let spent = l
            .rule
            .spec
            .max_hits
            .is_some_and(|m| l.rule.hits >= u64::from(m));
        if spent {
            inner.end(index, EndedWhy::Spent, wall);
            self.recount(&inner);
        }
        drop(inner);
        match effect {
            Effect::Slow { .. } => Verdict::Slow {
                delay: Duration::from_millis(wait_ms),
                tag,
            },
            Effect::Fail(fail) => Verdict::Fail(Box::new(codes::failure(&fail, ask, tag))),
            Effect::Drop => Verdict::Drop { tag },
        }
    }
}

#[cfg(test)]
mod tests;
