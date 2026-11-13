//! Needs you on the window (M2 step 2.4): the list (`doze_desk::attention`), built off the UI thread from the hub and
//! the facts each time the hub moves, kept steady by Go's rules; alerts for red items that appear while the window is
//! open (Go's `gotAttention`: the first build only primes, at most three, none while the bell is open); and what
//! happened while the window was closed, said once.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, SystemTime};

use doze_desk::StackView;
use doze_desk::attention::{
    self as attention, Attention, Item, MachinePart, StackFacts, Steady, away::Away,
};
use gpui::{AsyncApp, Context, Entity, WeakEntity};

use super::Root;
use crate::facts::Facts;

/// How long an item waiting out its grace or its "fixed" waits before the list is built again.
const LOOK_AGAIN: Duration = Duration::from_secs(3);

/// The window's Needs you.
pub(crate) struct Attn {
    facts: Entity<Facts>,
    steady: Steady,
    /// The list as last built.
    pub(crate) list: Attention,
    not_now: BTreeMap<String, String>,
    /// Every red item seen (`id|print`), so only a new one alerts; `None` until the first build primes it.
    seen: Option<HashSet<String>>,
    /// The alerts shown, oldest first.
    pub(crate) alerts: Vec<Item>,
    /// When the window last closed, until the first build says what happened since.
    away_since: Option<SystemTime>,
    pub(crate) away: Option<Away>,
    building: bool,
    dirty: bool,
    timer: bool,
    /// The machine's parts as a shot stages them, in place of doctor's.
    pub(crate) parts: Option<Vec<MachinePart>>,
}

impl Attn {
    pub(crate) fn new(facts: Entity<Facts>) -> Attn {
        Attn {
            facts,
            steady: Steady::default(),
            list: Attention::default(),
            not_now: crate::prefs::recall::not_now(),
            seen: None,
            alerts: Vec::new(),
            away_since: crate::prefs::recall::take_window_closed(),
            away: None,
            building: false,
            dirty: false,
            timer: false,
            parts: None,
        }
    }

    /// Puts an item aside until what it says changes.
    pub(super) fn put_aside(&mut self, it: &Item) {
        attention::not_now(&mut self.not_now, &it.id, Some(&it.print));
        crate::prefs::recall::save_not_now(&self.not_now);
        self.alerts.retain(|a| a.id != it.id);
    }

    /// Items of the stack `key` leave without "fixed": the person dismissed or removed them.
    pub(super) fn quiet(&mut self, key: &str) {
        self.steady.quiet(|i| i.key == key);
    }

    /// Takes a new list: what alerts, what is kept.
    fn take(&mut self, a: Attention, bell_open: bool) -> bool {
        let red: Vec<&Item> = a.items.iter().filter(|i| i.red && !i.fixed).collect();
        let print = |i: &Item| format!("{}|{}", i.id, i.print);
        let fresh: Vec<Item> = match &self.seen {
            Some(seen) => red
                .iter()
                .filter(|i| !seen.contains(&print(i)))
                .map(|i| (*i).clone())
                .collect(),
            None => Vec::new(),
        };
        let live: HashSet<&str> = red.iter().map(|i| i.id.as_str()).collect();
        let mut alerts: Vec<Item> = self
            .alerts
            .iter()
            .filter(|x| live.contains(x.id.as_str()))
            .map(|x| {
                red.iter()
                    .find(|i| i.id == x.id)
                    .map_or_else(|| x.clone(), |i| (*i).clone())
            })
            .collect();
        alerts.extend(
            fresh
                .into_iter()
                .filter(|f| !self.alerts.iter().any(|x| x.id == f.id)),
        );
        if bell_open {
            alerts.clear();
        }
        let skip = alerts.len().saturating_sub(3);
        alerts.drain(..skip);
        let seen = self.seen.get_or_insert_with(HashSet::new);
        seen.extend(red.iter().map(|i| print(i)));
        let changed = a != self.list || alerts != self.alerts;
        self.list = a;
        self.alerts = alerts;
        changed
    }
}

/// What is wrong now, stack by stack, then the machine's (off the UI thread: a crash's port is looked up with lsof).
fn items_now(stacks: &[StackView], parts: &[MachinePart], now: SystemTime) -> Vec<Item> {
    let held = |p: u16| doze_desk::ports::holder(p);
    let mut out = Vec::new();
    for st in stacks {
        let missing = doze_desk::library::missing_files(&st.dir);
        let mtime = std::fs::metadata(std::path::Path::new(&st.dir).join("doze.hcl"))
            .and_then(|m| m.modified())
            .ok();
        let changed = mtime.and_then(|m| doze_desk::overview::config_changed(m, st.loaded, now));
        let f = StackFacts {
            missing: &missing,
            changed: changed.as_ref(),
            changed_at: mtime,
            held: Some(&held),
            ..StackFacts::default()
        };
        out.extend(attention::stack_items(st, &f, now));
    }
    out.extend(attention::machine_items(parts, false, 0, 0));
    out
}

impl Root {
    /// Builds Needs you again (once at a time; a move during a build builds once more after it).
    pub(crate) fn rebuild_attention(&mut self, cx: &mut Context<Self>) {
        if self.attn.building {
            self.attn.dirty = true;
            return;
        }
        self.attn.building = true;
        let stacks = self.hub.stacks();
        let now = self.hub.now();
        let doctor = self.attn.facts.update(cx, |f, cx| f.doctor(&stacks, cx));
        let parts: Vec<MachinePart> = match self.attn.parts.clone() {
            Some(p) => p,
            None => doctor
                .map(|d| {
                    d.checks
                        .into_iter()
                        .map(|c| MachinePart {
                            section: c.section.into(),
                            name: c.name,
                            ok: c.ok,
                            text: c.text,
                            fix: String::new(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        let job = cx.background_executor().spawn(async move {
            let items = items_now(&stacks, &parts, now);
            (items, stacks)
        });
        cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
            let (items, stacks) = job.await;
            let _ = this.update(cx, |r, cx| r.took_attention(items, &stacks, now, cx));
        })
        .detach();
    }

    fn took_attention(
        &mut self,
        items: Vec<Item>,
        stacks: &[StackView],
        now: SystemTime,
        cx: &mut Context<Self>,
    ) {
        self.attn.building = false;
        let a = self
            .attn
            .steady
            .step(items, stacks, &|_| false, &self.attn.not_now, now);
        let bell_open = self.bell_open();
        if self.attn.take(a, bell_open) {
            self.bell_follow(cx);
            cx.notify();
        }
        if let Some(since) = self.attn.away_since.filter(|_| !stacks.is_empty()) {
            self.attn.away_since = None;
            self.attn.away = attention::away::away_of(stacks, since, now, crate::instance::zone());
            cx.notify();
        }
        if self.attn.dirty {
            self.attn.dirty = false;
            self.rebuild_attention(cx);
        } else if self.attn.steady.waiting() && !self.attn.timer {
            self.attn.timer = true;
            cx.spawn(async move |this: WeakEntity<Root>, cx: &mut AsyncApp| {
                cx.background_executor().timer(LOOK_AGAIN).await;
                let _ = this.update(cx, |r, cx| {
                    r.attn.timer = false;
                    r.rebuild_attention(cx);
                });
            })
            .detach();
        }
    }

    /// The red count of the stack `key`, for its tab's badge.
    pub(crate) fn red_of(&self, key: &doze_desk::StackKey) -> usize {
        self.attn.list.red_in(&key.dir().to_string_lossy())
    }
}
