//! The log ring: what a stack's instances printed, in arrival order, as much as a page can use (PLAN.md §3 item 4).
//!
//! Go kept 400 lines an instance and 800 merged, each a `LogFrame` of four strings. This keeps 100,000 lines a stack
//! in a compact form: the text of the lines in 64 KiB arenas, and per line one fixed 32-byte record (its time, where
//! its text is, its run, and its instance and stream as numbers into a table of names). A line is evicted when the
//! ring holds more than [`LINES_KEPT`] lines or more than [`TEXT_KEPT`] bytes of text, so a stack's logs never cost
//! more than about 24 MB however long its lines are. An arena is freed once every line in it is evicted.

mod line;
mod names;

pub use line::LineRef;

use doze_control::LogFrame;
use line::{Meta, nanos_of, time_of};
use names::Names;
use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
use std::ops::AddAssign;

/// How many lines a stack keeps.
pub const LINES_KEPT: usize = 100_000;
/// How many bytes of line text a stack keeps.
pub const TEXT_KEPT: usize = 20 * 1024 * 1024;
/// How many recent lines of each instance a new logs subscription asks for (Go's `linesKept`).
pub const BACKLOG: i64 = 400;
/// How many of each instance's latest lines a line from a reconnect's backlog is compared with.
const RECENT: usize = 512;
/// The size of a text arena.
const ARENA: usize = 64 * 1024;

/// A stack's log lines, oldest first.
#[derive(Debug, Default)]
pub struct LogRing {
    lines: VecDeque<Meta>,
    arenas: VecDeque<String>,
    /// The number of the first arena in `arenas`.
    first_arena: u32,
    /// How many lines were ever evicted: line `seq` is at `seq - evicted`.
    evicted: u64,
    text: usize,
    instances: Names,
    streams: Names,
    /// Lines held, by instance.
    held: Vec<usize>,
    /// Lines ever added, by instance; and in all.
    added: Vec<u64>,
    added_all: u64,
    /// Each instance's latest lines, by sequence number, for the reconnect check.
    recent: HashMap<u32, VecDeque<u64>>,
}

impl LogRing {
    /// An empty ring.
    pub fn new() -> LogRing {
        LogRing::default()
    }

    /// Adds a line unless it is one already held, and says whether it did: a reconnect's backlog repeats lines.
    /// Lines arrive in time order, so only the instance's lines as new as it are compared, as Go's ring did.
    pub fn push(&mut self, f: &LogFrame) -> bool {
        let nanos = nanos_of(&f.time);
        let run = u32::try_from(f.run.max(0)).unwrap_or(u32::MAX);
        let instance = self.instances.id(&f.instance);
        if self.holds(instance, nanos, run, &f.line) {
            return false;
        }
        let stream = self.streams.id(&f.stream);
        let (arena, start) = self.store(&f.line);
        let len = u32::try_from(f.line.len()).unwrap_or(u32::MAX);
        let seq = self.evicted + self.lines.len() as u64;
        if self.lines.len() == self.lines.capacity() {
            // Doubling, but never past what the ring holds: a full ring's records are exactly its lines'.
            let want = (self.lines.capacity() * 2).clamp(64, LINES_KEPT + 1);
            self.lines
                .reserve_exact(want.saturating_sub(self.lines.len()));
        }
        self.lines.push_back(Meta {
            nanos,
            arena,
            start,
            len,
            run,
            instance,
            stream,
        });
        self.text += f.line.len();
        bump(&mut self.held, instance);
        bump(&mut self.added, instance);
        self.added_all += 1;
        let recent = self.recent.entry(instance).or_default();
        recent.push_back(seq);
        if recent.len() > RECENT {
            recent.pop_front();
        }
        while self.lines.len() > LINES_KEPT || (self.text > TEXT_KEPT && self.lines.len() > 1) {
            self.evict();
        }
        true
    }

    fn holds(&self, instance: u32, nanos: i64, run: u32, line: &str) -> bool {
        let Some(recent) = self.recent.get(&instance) else {
            return false;
        };
        for &seq in recent.iter().rev() {
            let Some(m) = self.meta(seq) else { break };
            if m.nanos < nanos {
                break;
            }
            if m.nanos == nanos && m.run == run && self.text_of(m) == line {
                return true;
            }
        }
        false
    }

    /// Puts `line` in an arena; returns which and where.
    fn store(&mut self, line: &str) -> (u32, u32) {
        let fits = self
            .arenas
            .back()
            .is_some_and(|a| a.capacity() - a.len() >= line.len());
        if !fits {
            if self.arenas.back().is_some() {
                self.trim_last();
            }
            self.arenas
                .push_back(String::with_capacity(ARENA.max(line.len())));
        }
        let number = self.first_arena + u32::try_from(self.arenas.len() - 1).unwrap_or(0);
        let Some(a) = self.arenas.back_mut() else {
            return (number, 0);
        };
        let start = u32::try_from(a.len()).unwrap_or(u32::MAX);
        a.push_str(line);
        (number, start)
    }

    /// A full arena gives back what it will not use.
    fn trim_last(&mut self) {
        if let Some(a) = self.arenas.back_mut() {
            a.shrink_to_fit();
        }
    }

    fn evict(&mut self) {
        let Some(m) = self.lines.pop_front() else {
            return;
        };
        self.evicted += 1;
        self.text = self.text.saturating_sub(m.len as usize);
        if let Some(n) = self.held.get_mut(m.instance as usize) {
            *n = n.saturating_sub(1);
        }
        let first_live = self.lines.front().map_or(m.arena + 1, |f| f.arena);
        while self.first_arena < first_live && self.arenas.len() > 1 {
            self.arenas.pop_front();
            self.first_arena += 1;
        }
    }

    fn meta(&self, seq: u64) -> Option<&Meta> {
        let i = usize::try_from(seq.checked_sub(self.evicted)?).ok()?;
        self.lines.get(i)
    }

    fn text_of(&self, m: &Meta) -> &str {
        let i = (m.arena - self.first_arena) as usize;
        let (start, end) = (m.start as usize, m.start as usize + m.len as usize);
        self.arenas
            .get(i)
            .and_then(|a| a.get(start..end))
            .unwrap_or_default()
    }

    fn line(&self, m: &Meta) -> LineRef<'_> {
        LineRef {
            instance: self.instances.name(m.instance),
            line: self.text_of(m),
            stream: self.streams.name(m.stream),
            run: m.run,
            time: time_of(m.nanos),
        }
    }

    /// How many lines it holds.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The `i`th line held, oldest first.
    pub fn get(&self, i: usize) -> Option<LineRef<'_>> {
        self.lines.get(i).map(|m| self.line(m))
    }

    /// Every line held, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = LineRef<'_>> + '_ {
        self.lines.iter().map(|m| self.line(m))
    }

    /// The lines of one instance, oldest first.
    pub fn of<'a>(&'a self, instance: &str) -> impl DoubleEndedIterator<Item = LineRef<'a>> + 'a {
        let id = self.instances.find(instance);
        self.lines
            .iter()
            .filter(move |m| Some(m.instance) == id)
            .map(|m| self.line(m))
    }

    /// How many lines of `instance` it holds; `""` is every instance.
    pub fn held(&self, instance: &str) -> usize {
        if instance.is_empty() {
            return self.len();
        }
        self.instances
            .find(instance)
            .and_then(|id| self.held.get(id as usize).copied())
            .unwrap_or(0)
    }

    /// How many lines `instance` ever added (`""`: every instance), so a page can tell new lines from a ring that
    /// only slid (Go `LogsVersion`).
    pub fn added(&self, instance: &str) -> u64 {
        if instance.is_empty() {
            return self.added_all;
        }
        self.instances
            .find(instance)
            .and_then(|id| self.added.get(id as usize).copied())
            .unwrap_or(0)
    }

    /// The last `n` lines of `instance` (`""`: every instance), oldest first, as the daemon sent them.
    pub fn tail(&self, instance: &str, n: usize) -> Vec<LogFrame> {
        let mut out: Vec<LogFrame> = if instance.is_empty() {
            self.iter().rev().take(n).map(|l| l.to_frame()).collect()
        } else {
            self.of(instance)
                .rev()
                .take(n)
                .map(|l| l.to_frame())
                .collect()
        };
        out.reverse();
        out
    }

    /// The bytes it has taken from the heap: the arenas' capacity, the line records' and the tables'. What the
    /// ring costs, without the allocator's own overhead.
    pub fn heap_bytes(&self) -> usize {
        let arenas: usize = self.arenas.iter().map(String::capacity).sum();
        let lines = self.lines.capacity() * size_of::<Meta>();
        let counts = (self.held.capacity() + self.added.capacity()) * size_of::<u64>();
        let recent: usize = self
            .recent
            .values()
            .map(|r| r.capacity() * size_of::<u64>() + size_of::<VecDeque<u64>>())
            .sum();
        arenas + lines + counts + recent + self.instances.heap_bytes() + self.streams.heap_bytes()
    }
}

/// Adds one to the count of `id`, growing the table to hold it.
fn bump<T: Default + Clone + AddAssign + From<u8>>(v: &mut Vec<T>, id: u32) {
    let i = id as usize;
    if v.len() <= i {
        v.resize(i + 1, T::default());
    }
    if let Some(n) = v.get_mut(i) {
        *n += T::from(1);
    }
}
