//! Which lines a logs page shows, kept up as they arrive. Go filtered every kept line again on every push and sent
//! a window; here the ring holds 100,000 lines a stack, so the page keeps an index into it instead: the shown lines'
//! sequence numbers (and doze's own events), with the counts the toolbar shows. A new line costs one look; a change
//! of filter, one pass over the ring. A page draws only the rows in view, reading each from the ring through its
//! number ([`LogIndex::rows`]).
//!
//! The merged page is in time order, as Go's sorted it: a daemon sends each instance's backlog in turn, so lines do
//! not arrive in time order, and a batch that is older than what is shown is merged in where it belongs.
//!
//! In find mode nothing is hidden: the lines the filter matches are marked as hits to step through (`n`, `N`).

use super::filter::Filter;
use super::logs::{DOZE, DozeLine, rank_of};
use crate::ring::{LineRef, LogRing};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Marks an entry that is one of doze's own events (the rest is its place in `events`).
pub(super) const EVENT: u64 = 1 << 63;
/// Marks an entry the filter matched, in find mode.
pub(super) const HIT: u64 = 1 << 62;
/// What is left of an entry once its marks are off.
pub(super) const NUMBER: u64 = HIT - 1;
/// How many of doze's events a page holds before it starts again from the journal.
const EVENTS_HELD: usize = 4096;

const COUNTED: u8 = 1 << 2;
const MATCHED: u8 = 1 << 3;

/// Where a line or event sorts: its time (none first), lines before events at the same time, then arrival.
type Key = (i64, u8, u64);

/// What a logs page asks for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LogQuery {
    /// One instance's lines; empty for every instance merged, with doze's own events.
    pub instance: String,
    /// On the merged page, only these instances (`doze` for doze's own); `None` for all.
    pub show: Option<BTreeSet<String>>,
    /// Lines at this rank or above: 0 everything … 3 errors only.
    pub level: u8,
    pub filter: Filter,
    /// One run's lines; 0 for every run.
    pub run: u32,
    /// Find instead of filter: every line stays, the matches are hits.
    pub find: bool,
}

impl LogQuery {
    pub(super) fn merged(&self) -> bool {
        self.instance.is_empty()
    }

    fn showing(&self, inst: &str) -> bool {
        self.show.as_ref().is_none_or(|s| s.contains(inst))
    }

    fn filtering(&self) -> bool {
        self.filter.active() && !self.find
    }
}

/// The shown lines of a logs page, and what the toolbar says about them.
#[derive(Debug, Default)]
pub struct LogIndex {
    pub(super) query: LogQuery,
    /// The next ring line to look at.
    next: u64,
    /// The shown entries: a ring sequence number, or `EVENT | i`; `HIT` on a match in find mode.
    pub(super) entries: VecDeque<u64>,
    /// How many entries were ever dropped, so a place in the list survives lines sliding out.
    pub(super) dropped: u64,
    pub(super) hit_n: usize,
    /// Per ring line from `first`: its rank, and whether it is counted and matched.
    marks: VecDeque<u8>,
    first: u64,
    pub(super) counts: [usize; 4],
    pub(super) matches: usize,
    pub(super) runs: BTreeSet<u32>,
    /// Where each instance first appears; `instances` is them in that order.
    firsts: HashMap<String, Key>,
    pub(super) instances: Vec<String>,
    pub(super) events: Vec<DozeLine>,
}

impl LogIndex {
    /// An empty index for `query`; [`LogIndex::update`] fills it.
    pub fn new(query: LogQuery) -> LogIndex {
        LogIndex {
            query,
            ..LogIndex::default()
        }
    }

    pub fn query(&self) -> &LogQuery {
        &self.query
    }

    /// Asks something else: the index starts again on the next update.
    pub fn set_query(&mut self, query: LogQuery) {
        if query != self.query {
            *self = LogIndex::new(query);
        }
    }

    /// Takes in what arrived since the last update (lines from `ring`, events from `journal`) and drops what the
    /// ring let go. Says whether anything shown changed.
    pub fn update(&mut self, ring: &LogRing, journal: &[doze_control::Event]) -> bool {
        if self.events.len() > EVENTS_HELD {
            // A long-lived page: start again from the ring and the journal kept, which forgets the oldest events.
            *self = LogIndex::new(std::mem::take(&mut self.query));
        }
        let before = (self.dropped, self.entries.len(), self.counts, self.hit_n);
        self.evict(ring);
        let mut batch: Vec<(Key, u64)> = Vec::new();
        let mut firsts = false;
        for (seq, l) in ring.since(self.next) {
            let key = (nanos(l.time), 0, seq);
            firsts |= self.seen(
                l.instance,
                l.run,
                key,
                self.query.merged() || l.instance == self.query.instance,
            );
            if let Some(e) = self.take_line(seq, &l) {
                batch.push((key, e));
            }
        }
        self.next = ring.next_seq();
        for i in self.new_events(journal) {
            let key = (nanos(self.events.get(i).and_then(|e| e.time)), 1, i as u64);
            firsts |= self.seen(DOZE, 0, key, true);
            if let Some(e) = self.take_event(i) {
                batch.push((key, e));
            }
        }
        self.place(ring, batch);
        if firsts {
            let mut order: Vec<(&Key, &String)> = self.firsts.iter().map(|(n, k)| (k, n)).collect();
            order.sort();
            self.instances = order.into_iter().map(|(_, n)| n.clone()).collect();
        }
        firsts || before != (self.dropped, self.entries.len(), self.counts, self.hit_n)
    }

    /// doze's events not taken in yet, as indexes into `events`; none unless the merged page shows doze.
    fn new_events(&mut self, journal: &[doze_control::Event]) -> std::ops::Range<usize> {
        let start = self.events.len();
        if !self.query.merged() || !self.query.showing(DOZE) {
            return start..start;
        }
        let all: Vec<DozeLine> = journal.iter().map(DozeLine::of).collect();
        let fresh = match self.events.last() {
            None => 0,
            Some(last) => all.iter().rposition(|e| e == last).map_or(0, |i| i + 1),
        };
        self.events.extend(all.into_iter().skip(fresh));
        start..self.events.len()
    }

    /// Puts a batch of new entries in: at the end, or (on the merged page, when the batch is older than the end)
    /// merged in by time, the entries already shown first at the same time.
    fn place(&mut self, ring: &LogRing, mut batch: Vec<(Key, u64)>) {
        if batch.is_empty() {
            return;
        }
        if !self.query.merged() {
            self.entries.extend(batch.into_iter().map(|(_, e)| e));
            return;
        }
        batch.sort_by_key(|(k, _)| *k);
        let last = self.entries.back().map(|&e| self.key_of(ring, e));
        if last.is_none_or(|last| batch.first().is_none_or(|(k, _)| *k >= last)) {
            self.entries.extend(batch.into_iter().map(|(_, e)| e));
            return;
        }
        let old: Vec<(Key, u64)> = self
            .entries
            .iter()
            .map(|&e| (self.key_of(ring, e), e))
            .collect();
        self.entries.clear();
        let (mut a, mut b) = (old.into_iter().peekable(), batch.into_iter().peekable());
        loop {
            let take_old = match (a.peek(), b.peek()) {
                (Some(x), Some(y)) => x.0 <= y.0,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            let next = if take_old { a.next() } else { b.next() };
            if let Some((_, e)) = next {
                self.entries.push_back(e);
            }
        }
    }

    fn key_of(&self, ring: &LogRing, e: u64) -> Key {
        let n = e & NUMBER;
        if e & EVENT != 0 {
            let t = usize::try_from(n)
                .ok()
                .and_then(|i| self.events.get(i))
                .and_then(|d| d.time);
            (nanos(t), 1, n)
        } else {
            (nanos(ring.at(n).and_then(|l| l.time)), 0, n)
        }
    }

    /// Counts a line in, and says whether it is shown (as what entry).
    fn take_line(&mut self, seq: u64, l: &LineRef<'_>) -> Option<u64> {
        let q = &self.query;
        let mine = q.merged() || l.instance == q.instance;
        let (level, _) = doze_display::rank(l.line);
        let rank = rank_of(level);
        let shown = mine && q.showing(l.instance);
        let matched = shown && q.filter.active() && q.filter.keeps(l.line);
        let keep = shown
            && rank >= q.level
            && (q.run == 0 || l.run == q.run)
            && (!q.filtering() || matched);
        let hit = matched && q.find;
        if self.marks.is_empty() {
            self.first = seq;
        }
        let mut mark = rank;
        if mine {
            mark |= COUNTED;
            bump(&mut self.counts, mark);
        }
        if matched {
            mark |= MATCHED;
            self.matches += 1;
        }
        self.marks.push_back(mark);
        keep.then(|| self.entry(seq, hit))
    }

    fn take_event(&mut self, i: usize) -> Option<u64> {
        bump(&mut self.counts, 1);
        let q = &self.query;
        let e = self.events.get(i)?;
        let matched = q.filter.active() && q.filter.keeps(&e.text);
        let keep = q.level <= 1 && q.run == 0 && (!q.filtering() || matched);
        let hit = matched && q.find;
        self.matches += usize::from(matched);
        keep.then(|| self.entry(EVENT | i as u64, hit))
    }

    fn entry(&mut self, e: u64, hit: bool) -> u64 {
        if hit {
            self.hit_n += 1;
            e | HIT
        } else {
            e
        }
    }

    /// Notes an instance's run and where it first appears; says whether that moved.
    fn seen(&mut self, inst: &str, run: u32, key: Key, mine: bool) -> bool {
        if !mine {
            return false;
        }
        if run > 0 {
            self.runs.insert(run);
        }
        match self.firsts.get_mut(inst) {
            Some(k) if *k <= key => false,
            Some(k) => {
                *k = key;
                true
            }
            None => {
                self.firsts.insert(inst.to_owned(), key);
                true
            }
        }
    }

    /// Drops what the ring let go: its marks, its counts, and the entries that showed it.
    fn evict(&mut self, ring: &LogRing) {
        let first = ring.first_seq();
        if self.first >= first {
            return;
        }
        while self.first < first {
            let Some(m) = self.marks.pop_front() else {
                self.first = first;
                break;
            };
            self.first += 1;
            if m & COUNTED != 0
                && let Some(c) = self.counts.get_mut(usize::from(m & 3))
            {
                *c = c.saturating_sub(1);
            }
            if m & MATCHED != 0 {
                self.matches = self.matches.saturating_sub(1);
            }
        }
        let gone = |e: u64| e & EVENT == 0 && e & NUMBER < first;
        let hits = self
            .entries
            .iter()
            .filter(|&&e| gone(e) && e & HIT != 0)
            .count();
        let n = self.entries.len();
        self.entries.retain(|&e| !gone(e));
        self.dropped += (n - self.entries.len()) as u64;
        self.hit_n -= hits.min(self.hit_n);
    }
}

/// A time as nanoseconds since the epoch; none sorts first.
fn nanos(t: Option<SystemTime>) -> i64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(i64::MIN)
}

/// One more line at a level.
fn bump(counts: &mut [usize; 4], mark: u8) {
    if let Some(c) = counts.get_mut(usize::from(mark & 3)) {
        *c += 1;
    }
}
