//! Needs you on the window (M2 step 2.4): the list (`doze_desk::attention`), built off the UI thread from the hub and
//! the facts each time the hub moves, kept steady by Go's rules; alerts for red items that appear while the window is
//! open (Go's `gotAttention`: the first build only primes, at most three, none while the bell is open); and what
//! happened while the window was closed, said once. What acting knows goes in too: a stack action that failed is an
//! item until dismissed or tried again, and a stack mid-action carries its items as they were (Go's steadiness).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, SystemTime};

use doze_desk::StackView;
use doze_desk::attention::{
    self as attention, Attention, Item, MachinePart, StackFacts, StackFailure, Steady, away::Away,
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
    /// Where the `doze` is (the app's own part of the machine), looked for again with each new doctor report (Go: with
    /// each `doctor`), by the report's number.
    at: Option<(u64, doze_desk::settings::machine::DozeAt)>,
    /// Acting's facts as last built from: the stacks mid-action, and each failed action's words, by stack.
    acting: (BTreeSet<String>, BTreeMap<String, String>),
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
            at: None,
            acting: (BTreeSet::new(), BTreeMap::new()),
        }
    }

    /// Puts an item aside until what it says changes.
    pub(super) fn put_aside(&mut self, it: &Item) {
        attention::not_now(&mut self.not_now, &it.id, Some(&it.print));
        crate::prefs::recall::save_not_now(&self.not_now);
        self.alerts.retain(|a| a.id != it.id);
    }

    /// Items of the stack `key` leave without "fixed": the person dismissed or removed them.
    pub(crate) fn quiet(&mut self, key: &str) {
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
/// What the machine adds: its parts, the loader's words for each running stack whose doze.hcl does not load (by
/// folder), and what stacks left behind.
#[derive(Debug, Clone, Default)]
pub(crate) struct Machine {
    pub(crate) parts: Vec<MachinePart>,
    pub(crate) broken: HashMap<String, String>,
    /// The folders whose doze.hcl was saved a moment ago and is being read again.
    pub(crate) checking: HashSet<String>,
    /// What stacks left behind, counted as Settings › Machine lists them: what Clean up clears, and the data
    /// folders (deleted one at a time, after asking).
    pub(crate) left: (usize, usize),
    /// This app can ask for the administrator's password, so a part not in place offers Set up (Repair).
    pub(crate) repairable: bool,
}

pub(crate) fn items_now(
    stacks: &[StackView],
    failed: &HashMap<String, StackFailure>,
    m: &Machine,
    now: SystemTime,
) -> Vec<Item> {
    let held = |p: u16| doze_desk::ports::holder(p);
    let mut out = Vec::new();
    for st in stacks {
        let missing = doze_desk::library::missing_files(&st.dir);
        let mtime = std::fs::metadata(std::path::Path::new(&st.dir).join("doze.hcl"))
            .and_then(|m| m.modified())
            .ok();
        // A running stack's doze.hcl that does not load is the item; "changed" only when it loads (Go).
        let file = std::path::Path::new(&st.dir).join("doze.hcl");
        let config = m
            .broken
            .get(&st.dir)
            .filter(|_| st.running)
            .map(|err| doze_desk::overview::config_error(&file, err, mtime, now));
        let changed = mtime
            .and_then(|t| doze_desk::overview::config_changed(t, st.loaded, now))
            .filter(|_| config.is_none() && !m.checking.contains(&st.dir));
        let f = StackFacts {
            failed: failed.get(&*st.key.dir().to_string_lossy()),
            config: config.as_ref(),
            missing: &missing,
            changed: changed.as_ref(),
            changed_at: mtime,
            held: Some(&held),
        };
        out.extend(attention::stack_items(st, &f, now));
    }
    // Repair asks for the administrator's password through the system's prompt where it can (Settings › Machine's
    // Repair); elsewhere a part offers its command to copy.
    out.extend(attention::machine_items(
        &m.parts,
        m.repairable,
        m.left.0,
        m.left.1,
    ));
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
        let (busy, failed) = self.acting_now(cx);
        self.attn.acting = (
            busy.clone(),
            failed.iter().map(|(k, f)| (k.clone(), f.said())).collect(),
        );
        let stacks = self.hub.stacks();
        let now = self.hub.now();
        let home = self.hub.home().to_path_buf();
        let (doctor, broken, left, (live, seen)) = self.attn.facts.update(cx, |f, cx| {
            // A running stack's doze.hcl is read again when saved (the CLI's word on it, as the page has it).
            let broken: HashMap<String, String> = stacks
                .iter()
                .filter(|s| s.running)
                .filter_map(|s| Some((s.dir.clone(), f.shape(&s.dir, cx)?.error?)))
                .collect();
            let checking = stacks
                .iter()
                .filter(|s| f.checking(&s.dir))
                .map(|s| s.dir.clone())
                .collect();
            (
                f.doctor(&stacks, cx),
                (broken, checking),
                f.leftovers(&home, &stacks, cx)
                    .map(|l| doze_desk::settings::leftovers::counts(&l))
                    .unwrap_or_default(),
                (f.live(), f.doctor_seen()),
            )
        });
        let (broken, checking) = broken;
        self.tell_broken(&broken, now, cx);
        let parts: Vec<MachinePart> = match self.attn.parts.clone() {
            Some(p) => p,
            None => {
                if self.attn.at.as_ref().is_none_or(|(n, _)| *n != seen) {
                    self.attn.at = Some((seen, crate::settings::doze_at(!live)));
                }
                let at = self
                    .attn
                    .at
                    .as_ref()
                    .map(|(_, a)| a)
                    .cloned()
                    .unwrap_or_default();
                doctor
                    .map(|d| doze_desk::settings::machine::attention_parts(&d, &at, crate::VERSION))
                    .unwrap_or_default()
            }
        };
        let machine = Machine {
            parts,
            broken,
            checking,
            left,
            repairable: doze_os::privilege::admin_supported(),
        };
        let job = cx.background_executor().spawn(async move {
            let items = items_now(&stacks, &failed, &machine, now);
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
        // The menu bar's own list steps with the same items, so it is steady when the window closes (M3).
        let raw = items.clone();
        let a = self.attn.steady.step(
            items,
            stacks,
            &|k| self.attn.acting.0.contains(k),
            &self.attn.not_now,
            now,
        );
        let bell_open = self.bell_open();
        let changed = self.attn.take(a, bell_open);
        // A failed stack action whose page is on screen: its banner says it, so no alert (the bell keeps the item).
        let shown = self.key().map(|k| k.dir().to_string_lossy().into_owned());
        let before = self.attn.alerts.len();
        self.attn
            .alerts
            .retain(|it| !(it.kind == "failed" && shown.as_deref() == Some(it.key.as_str())));
        // The menu bar and the notifications read the window's list while it is open (M3).
        let busy = self.attn.acting.0.clone();
        crate::around::window_list(&self.attn.list, raw, stacks, &busy, now, cx);
        if changed || self.attn.alerts.len() != before {
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

    /// What acting knows now: the stacks mid-action, and the failed actions, by stack folder (Needs you's keys).
    fn acting_now(&self, cx: &Context<Self>) -> (BTreeSet<String>, HashMap<String, StackFailure>) {
        let a = self.acts.read(cx);
        let dir = |k: &doze_desk::StackKey| k.dir().to_string_lossy().into_owned();
        (
            a.busy.keys().map(dir).collect(),
            a.failed.iter().map(|(k, f)| (dir(k), f.clone())).collect(),
        )
    }

    /// Acting moved: Needs you is built again only when a stack began or ended an action, or a failure came or went
    /// (a toast alone changes nothing here).
    pub(crate) fn acting_moved(&mut self, cx: &mut Context<Self>) {
        let (busy, failed) = self.acting_now(cx);
        let said: BTreeMap<String, String> =
            failed.iter().map(|(k, f)| (k.clone(), f.said())).collect();
        if (busy, said) != self.attn.acting {
            self.rebuild_attention(cx);
        }
    }
}
