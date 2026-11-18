//! The table of fault rules: bounded, in memory, and the one place a rule is made, changed, fired or ended.
//!
//! A request costs one relaxed load while no rule is switched on. With rules, the table is locked for the few comparisons a request
//! needs; it never holds the lock across an await, a clock read is the only system call, and nothing grows without a bound: 50 rules,
//! 200 ended ones, 8 client names a rule.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::matching::Ask;
use super::rule::{
    AddError, Effect, Ended, EndedWhy, FirstHit, LastHit, MOST_CLIENTS, MOST_RULES, Rule, Share,
    Spec,
};
use super::targets;
use super::{Act, Applied, FaultPlan, Note, Verdict};

/// How many ended rules are kept.
pub const MOST_ENDED: usize = 200;

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
    /// Requests that fitted since it last fired (for `EveryNth`).
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
    /// A number from a splitmix64 generator: seeded, so a test sees the same ones.
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
pub struct FaultTable {
    switched_on: AtomicUsize,
    inner: Mutex<Inner>,
    clock: Box<dyn Clock>,
}

impl fmt::Debug for FaultTable {
    /// Counts only: nothing a rule matches on is printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultTable")
            .field("on", &self.switched_on.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl FaultTable {
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

    /// Ends every rule whose time is up.
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

    /// Makes a rule. One with the same matcher and the same kind of effect is replaced.
    ///
    /// # Errors
    /// The rule is not allowed ([`Spec::check`]), or there are already 50.
    pub fn add(&self, spec: Spec) -> Result<u32, AddError> {
        spec.check()?;
        let now = self.clock.mono_ms();
        let wall = self.clock.wall_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        if let Some(i) = inner.rules.iter().position(|l| l.rule.spec.same_as(&spec)) {
            inner.end(i, EndedWhy::Replaced, wall);
        }
        if inner.rules.len() >= MOST_RULES {
            self.recount(&inner);
            return Err(AddError::TooMany);
        }
        let id = inner.next_id;
        inner.next_id = inner.next_id.wrapping_add(1).max(1);
        let ttl_ms = u64::try_from(spec.ttl.as_millis()).unwrap_or(u64::MAX);
        let rule = Rule {
            id,
            name: spec.display_name().into(),
            created_ms: wall,
            ends_ms: wall.saturating_add(i64::try_from(ttl_ms).unwrap_or(i64::MAX)),
            enabled: true,
            matched: 0,
            hits: 0,
            first: None,
            last: None,
            clients: Vec::new(),
            more_clients: 0,
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

    /// Ends a rule now. `false`: there is none with that number (it may have ended already).
    pub fn remove(&self, id: u32) -> bool {
        let now = self.clock.mono_ms();
        let wall = self.clock.wall_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let found = inner.rules.iter().position(|l| l.rule.id == id);
        if let Some(i) = found {
            inner.end(i, EndedWhy::Cleared, wall);
            self.recount(&inner);
        }
        found.is_some()
    }

    /// Switches a rule on or off; it runs out all the same. `false`: no such rule.
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

    /// One rule in force.
    #[must_use]
    pub fn rule(&self, id: u32) -> Option<Rule> {
        self.rules().into_iter().find(|r| r.id == id)
    }

    /// The rules that are over, newest first (at most [`MOST_ENDED`]).
    #[must_use]
    pub fn ended(&self) -> Vec<Ended> {
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        inner.ended.iter().cloned().collect()
    }

    /// Forgets the rules that are over.
    pub fn clear_ended(&self) {
        self.lock().ended.clear();
    }

    /// How many rules are in force, switched on or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules().len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The time, in milliseconds since the epoch, on the table's clock.
    #[must_use]
    pub fn now_ms(&self) -> i64 {
        self.clock.wall_ms()
    }
}

impl FaultPlan for FaultTable {
    fn judge(&self, ask: &Ask<'_>) -> Verdict {
        if self.switched_on.load(Ordering::Relaxed) == 0 {
            return Verdict::Pass;
        }
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let Some((index, applied)) = choose(&mut inner, ask) else {
            return Verdict::Pass;
        };
        let wall = self.clock.wall_ms();
        let note = record(&mut inner, index, ask, wall, applied);
        if inner.rules.get(index).is_some_and(spent) {
            inner.end(index, EndedWhy::Spent, wall);
            self.recount(&inner);
        }
        Verdict::Act(Act {
            effect: applied,
            note,
        })
    }
}

fn spent(l: &Live) -> bool {
    l.rule
        .spec
        .max_hits
        .is_some_and(|m| l.rule.hits >= u64::from(m))
}

/// Counts the request against every rule that fits it, and picks the rule that acts: a drop before a fail before a delay, the
/// longer delay of two.
fn choose(inner: &mut Inner, ask: &Ask<'_>) -> Option<(usize, Applied)> {
    let mut best: Option<(usize, Applied)> = None;
    for i in 0..inner.rules.len() {
        let fires = inner
            .rules
            .get(i)
            .is_some_and(|l| l.rule.enabled && l.rule.spec.matcher.fits(ask));
        if !fires {
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
        let applied = applied(l.rule.spec.effect, draw);
        if best
            .as_ref()
            .is_none_or(|(_, b)| severity(applied) > severity(*b))
        {
            best = Some((i, applied));
        }
    }
    best
}

/// Whether the share lets this request through.
fn gate(l: &mut Live, draw: u64) -> bool {
    match l.rule.spec.share {
        Share::Every => true,
        Share::EveryNth(n) => {
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

fn applied(effect: Effect, draw: u64) -> Applied {
    match effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => {
            let extra = if jitter_ms == 0 {
                0
            } else {
                u32::try_from((draw >> 16) % (u64::from(jitter_ms) + 1)).unwrap_or(0)
            };
            Applied::Delay(Duration::from_millis(u64::from(delay_ms + extra)))
        }
        Effect::Fail { code } => Applied::Fail(code),
        Effect::Drop => Applied::Drop,
    }
}

/// Which of two effects wins.
fn severity(a: Applied) -> (u8, u128) {
    match a {
        Applied::Drop => (3, 0),
        Applied::Fail(_) => (2, 0),
        Applied::Delay(d) => (1, d.as_millis()),
    }
}

/// Puts the hit in the rule's record, and makes the note the request carries on.
fn record(
    inner: &mut Inner,
    index: usize,
    ask: &Ask<'_>,
    wall: i64,
    applied: Applied,
) -> Arc<Note> {
    let Some(l) = inner.rules.get_mut(index) else {
        return Arc::new(Note {
            rule: 0,
            name: Arc::from(""),
            applied,
        });
    };
    let rule = &mut l.rule;
    rule.hits += 1;
    rule.last = Some(LastHit {
        at_ms: wall,
        conn: ask.conn,
        correlation: ask.head.correlation_id,
    });
    let client: Box<str> = ask
        .head
        .client_id
        .as_ref()
        .map_or("", |c| c.as_str())
        .chars()
        .take(64)
        .collect::<String>()
        .into();
    if rule.first.is_none() {
        rule.first = Some(FirstHit {
            at_ms: wall,
            client: client.clone(),
            api: ask.head.key,
            version: ask.head.version,
            target: ask
                .request
                .and_then(|r| targets::first_target(r, ask.names)),
        });
    }
    if !client.is_empty() && !rule.clients.contains(&client) {
        if rule.clients.len() < MOST_CLIENTS {
            rule.clients.push(client);
        } else {
            rule.more_clients = rule.more_clients.saturating_add(1);
        }
    }
    Arc::new(Note {
        rule: rule.id,
        name: Arc::clone(&rule.name),
        applied,
    })
}

#[cfg(test)]
mod tests;
