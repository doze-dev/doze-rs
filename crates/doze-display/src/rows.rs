//! How a stack is listed: the groups a surface puts instances under, the order within them, and the counts every
//! surface shows. The CLI's table and the app's pages list and count the same stack the same way because both take
//! it from here.

use std::collections::{HashMap, HashSet};

use crate::look::{Look, awake_now, look};
use crate::model::{Instance, Kind, Snapshot};
use crate::reach::connections;

/// A heading over part of the stack, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// Your code.
    Apps,
    /// From the doze registry (and what declared no kind).
    Modules,
    /// Brought in by you.
    Outside,
    /// Run once, then out of the way.
    Tasks,
}

impl Group {
    /// The groups, in the order they are drawn.
    pub const ALL: [Self; 4] = [Self::Apps, Self::Modules, Self::Outside, Self::Tasks];

    /// The heading.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Apps => "APPS",
            Self::Modules => "MODULES",
            Self::Outside => "OUTSIDE",
            Self::Tasks => "TASKS",
        }
    }

    /// A line on what the group is.
    #[must_use]
    pub fn what(self) -> &'static str {
        match self {
            Self::Apps => "your code",
            Self::Modules => "from the doze registry",
            Self::Outside => "brought in by you",
            Self::Tasks => "run once, then out of the way",
        }
    }

    /// The group an instance of `kind` is listed under; `None` for a kind no group holds.
    #[must_use]
    pub fn of(kind: Kind) -> Option<Self> {
        match kind {
            Kind::App => Some(Self::Apps),
            Kind::Module | Kind::Unspecified => Some(Self::Modules),
            Kind::Service => Some(Self::Outside),
            Kind::Task => Some(Self::Tasks),
            Kind::Other => None,
        }
    }
}

/// One instance in display order, and its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    /// The instance.
    pub v: &'a Instance,
    /// Its group.
    pub group: Group,
}

/// The stack listed: by group, then by name. (Go: `Rows`.)
#[must_use]
pub fn rows(snap: &Snapshot) -> Vec<Row<'_>> {
    let mut out: Vec<Row<'_>> = snap
        .instances
        .iter()
        .filter_map(|v| Group::of(v.kind).map(|group| Row { v, group }))
        .collect();
    out.sort_by(|a, b| a.group.cmp(&b.group).then_with(|| a.v.name.cmp(&b.v.name)));
    out
}

/// The orders a list can be sorted in. The groups stay; a sort orders the rows within each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortBy {
    /// By name.
    Name,
    /// By memory.
    Mem,
    /// By state.
    State,
}

impl SortBy {
    /// Every order, as a key bar cycles them.
    pub const ALL: [Self; 3] = [Self::Name, Self::Mem, Self::State];

    /// Its name, for a key bar or a menu. (Go: `SortTitles`.)
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Mem => "mem",
            Self::State => "state",
        }
    }
}

/// `rows` as the dependency tree: what nothing needs is a root, and what it needs is under it, once, at the first
/// place it is needed. Also how far down each is, by name. (Go: `TreeOrder`.)
#[must_use]
pub fn tree_order<'a>(rows: &[Row<'a>]) -> (Vec<Row<'a>>, HashMap<&'a str, usize>) {
    let by_name: HashMap<&str, Row<'a>> = rows.iter().map(|r| (r.v.name.as_str(), *r)).collect();
    let needed: HashSet<&str> = rows
        .iter()
        .flat_map(|r| &r.v.needs)
        .map(String::as_str)
        .filter(|n| by_name.contains_key(n))
        .collect();
    let mut out = Vec::with_capacity(rows.len());
    let mut depth = HashMap::with_capacity(rows.len());
    let roots = rows.iter().filter(|r| !needed.contains(r.v.name.as_str()));
    // then a cycle, or what only a cycle needs
    for root in roots.chain(rows) {
        let mut stack = vec![(*root, 0)];
        while let Some((r, d)) = stack.pop() {
            if depth.contains_key(r.v.name.as_str()) {
                continue;
            }
            depth.insert(r.v.name.as_str(), d);
            out.push(r);
            stack.extend(
                r.v.needs
                    .iter()
                    .rev()
                    .filter_map(|n| by_name.get(n.as_str()))
                    .map(|c| (*c, d + 1)),
            );
        }
    }
    (out, depth)
}

/// A stack's services counted by the look each one shows, tasks aside, with the memory and client connections of
/// the ones serving. Every surface that counts (the overview's strip, the library row, the menu bar) counts with
/// this, so the same stack never reads "4 awake" in one place and "7 awake" in another.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    /// Awake.
    pub awake: usize,
    /// Dozing.
    pub dozing: usize,
    /// Waking.
    pub waking: usize,
    /// Asleep.
    pub asleep: usize,
    /// Off.
    pub off: usize,
    /// Paused.
    pub paused: usize,
    /// Crashing or failed.
    pub trouble: usize,
    /// Tasks whose last run failed; they are trouble too.
    pub failed: usize,
    /// Resident bytes of the ones serving.
    pub ram: u64,
    /// Client connections to the ones serving.
    pub conns: u64,
}

/// Counts instances. (Go: `TallyOf`.)
#[must_use]
pub fn tally<'a>(vs: impl IntoIterator<Item = &'a Instance>) -> Tally {
    let mut t = Tally::default();
    for v in vs {
        if v.kind == Kind::Task {
            t.failed += usize::from(v.last_run_failed());
            continue;
        }
        let count = match look(v) {
            Look::Awake => &mut t.awake,
            Look::Dozing => &mut t.dozing,
            Look::Waking => &mut t.waking,
            Look::Off => &mut t.off,
            Look::Paused => &mut t.paused,
            Look::Crashing | Look::Failed => &mut t.trouble,
            Look::Asleep | Look::Running | Look::Done | Look::NotRun => &mut t.asleep,
        };
        *count += 1;
        if awake_now(v) {
            t.ram = t.ram.saturating_add(v.ram);
            t.conns += u64::from(connections(v));
        }
    }
    t
}
