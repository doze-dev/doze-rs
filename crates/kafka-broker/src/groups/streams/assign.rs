//! The sticky task assignor (Kafka 4.4.0's `StickyTaskAssignor`, named `sticky`, the only one `group.streams.assignors`
//! registers): active tasks stay where they run, then go to a member that has their state, then to the least loaded
//! process; standbys never go to a process that has the task, first back to where they were, then to the least loaded.
//!
//! The load of a process is its tasks over its members (a process with two members carries twice as much). Kafka breaks
//! ties between equally loaded processes, and members of a process, by the order of a hash map; here the tie goes to the
//! process with more members, then the smaller process id, and to the smaller member id. Kafka's own tests hold it to
//! counts and properties where there is a tie; these choices meet them and are the same every time.
//!
//! No warm-up task is made: Kafka 4.4.0's assignor makes none and its `AssignmentRefiner` passes the target through.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use kafka_wire::ids::MemberId;

use super::tasks::{Task, Tasks};

/// A subtopology as the assignor sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubTasks {
    pub(crate) tasks: i32,
    /// It has a changelog: its tasks get standbys.
    pub(crate) stateful: bool,
}

/// A member as the assignor sees it.
#[derive(Debug, Clone)]
pub(crate) struct Seat<'a> {
    pub(crate) id: &'a MemberId,
    pub(crate) process: &'a str,
    /// What it holds now.
    pub(crate) current: &'a Tasks,
    /// The changelog offsets it reported, by task.
    pub(crate) offsets: &'a BTreeMap<Task, i64>,
}

/// Kafka's `computeTasksPerMember`: the tasks over the members, rounded up; 0 with nobody.
fn per_member(tasks: usize, members: usize) -> usize {
    if members == 0 {
        0
    } else {
        tasks.div_ceil(members)
    }
}

struct Process {
    members: BTreeMap<MemberId, usize>,
    count: usize,
    has: BTreeSet<Task>,
}

impl Process {
    /// Compares two loads (tasks over members) exactly.
    fn lighter(&self, other: &Self) -> Ordering {
        let mine = self.count * other.members.len();
        let theirs = other.count * self.members.len();
        mine.cmp(&theirs)
            .then_with(|| other.members.len().cmp(&self.members.len()))
    }

    fn least_loaded_member(&self) -> Option<&MemberId> {
        self.members
            .iter()
            .min_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(b.0)))
            .map(|(id, _)| id)
    }
}

/// The quotas, kept as Kafka keeps them: a member that reaches one leaves the count it is shared between.
struct Quota {
    tasks: usize,
    members: usize,
    each: usize,
}

impl Quota {
    fn new(tasks: usize, members: usize) -> Self {
        Self {
            tasks,
            members,
            each: per_member(tasks, members),
        }
    }

    /// A member now has `count` tasks.
    fn note(&mut self, count: usize) {
        if count == self.each {
            self.members = self.members.saturating_sub(1);
            self.tasks = self.tasks.saturating_sub(count);
            self.each = per_member(self.tasks, self.members);
        }
    }
}

struct State<'a> {
    processes: BTreeMap<&'a str, Process>,
    process_of: BTreeMap<&'a MemberId, &'a str>,
    out: BTreeMap<MemberId, Tasks>,
    active: Quota,
    total: Quota,
    prev_active: BTreeMap<Task, &'a MemberId>,
    prev_standby: BTreeMap<Task, Vec<&'a MemberId>>,
}

impl<'a> State<'a> {
    fn count(&self, id: &MemberId) -> usize {
        self.process_of
            .get(id)
            .and_then(|p| self.processes.get(p))
            .and_then(|p| p.members.get(id))
            .copied()
            .unwrap_or(0)
    }

    fn process(&self, id: &MemberId) -> Option<&Process> {
        self.process_of.get(id).and_then(|p| self.processes.get(p))
    }

    /// Gives `task` to `id`; its new count of tasks.
    fn add(&mut self, id: &MemberId, task: &Task, active: bool) -> usize {
        let Some(process) = self
            .process_of
            .get(id)
            .and_then(|p| self.processes.get_mut(p))
        else {
            return 0;
        };
        process.count += 1;
        process.has.insert(task.clone());
        let count = process.members.entry(id.clone()).or_default();
        *count += 1;
        let tasks = self.out.entry(id.clone()).or_default();
        if active {
            tasks.active.insert(task.clone());
        } else {
            tasks.standby.insert(task.clone());
        }
        *count
    }

    fn add_active(&mut self, id: &MemberId, task: &Task) {
        let n = self.add(id, task, true);
        self.active.note(n);
        self.total.note(n);
    }

    fn add_standby(&mut self, id: &MemberId, task: &Task) {
        let n = self.add(id, task, false);
        self.total.note(n);
    }

    /// Kafka's `findPrevMemberWithLeastLoad`: the first candidate, replaced only by one both on a lighter process and
    /// with fewer tasks; for a standby, none on a process that has the task.
    fn least_loaded_of(
        &self,
        candidates: &[&'a MemberId],
        standby: Option<&Task>,
    ) -> Option<&'a MemberId> {
        let mut best: Option<&'a MemberId> = None;
        for &m in candidates {
            let Some(p) = self.process(m) else { continue };
            if standby.is_some_and(|t| p.has.contains(t)) {
                continue;
            }
            let better = best.is_none_or(|b| {
                self.process(b).is_some_and(|bp| {
                    p.lighter(bp) == Ordering::Less && self.count(m) < self.count(b)
                })
            });
            if better {
                best = Some(m);
            }
        }
        best
    }

    /// The least loaded process, of those not yet tried.
    fn lightest(&self, skip: &BTreeSet<&'a str>) -> Option<&'a str> {
        self.processes
            .iter()
            .filter(|(id, _)| !skip.contains(*id))
            .min_by(|a, b| a.1.lighter(b.1).then_with(|| a.0.cmp(b.0)))
            .map(|(id, _)| *id)
    }

    fn member_in(&self, process: &str) -> Option<MemberId> {
        self.processes
            .get(process)
            .and_then(Process::least_loaded_member)
            .cloned()
    }
}

/// Assigns every task of `subtopologies` over `seats`, with `standbys` standby replicas of each stateful task.
#[must_use]
pub(crate) fn assign(
    subtopologies: &BTreeMap<String, SubTasks>,
    seats: &[Seat<'_>],
    standbys: usize,
) -> BTreeMap<MemberId, Tasks> {
    let tasks = |stateful_only: bool| -> Vec<Task> {
        subtopologies
            .iter()
            .filter(|(_, s)| !stateful_only || s.stateful)
            .flat_map(|(id, s)| (0..s.tasks).map(move |p| (id.clone(), p)))
            .collect()
    };
    let (actives, stateful) = (tasks(false), tasks(true));
    let members = seats.len();
    let mut state = State {
        processes: BTreeMap::new(),
        process_of: BTreeMap::new(),
        out: seats
            .iter()
            .map(|s| (s.id.clone(), Tasks::default()))
            .collect(),
        active: Quota::new(actives.len(), members),
        total: Quota::new(actives.len() + stateful.len() * standbys, members),
        prev_active: BTreeMap::new(),
        prev_standby: BTreeMap::new(),
    };
    let mut candidates: BTreeMap<Task, Vec<(bool, i64, &MemberId)>> = BTreeMap::new();
    for s in seats {
        let p = state.processes.entry(s.process).or_insert_with(|| Process {
            members: BTreeMap::new(),
            count: 0,
            has: BTreeSet::new(),
        });
        p.members.insert(s.id.clone(), 0);
        state.process_of.insert(s.id, s.process);
        for t in &s.current.active {
            state.prev_active.insert(t.clone(), s.id);
        }
        for t in &s.current.standby {
            let offset = s.offsets.get(t).copied().unwrap_or(0);
            candidates
                .entry(t.clone())
                .or_default()
                .push((true, offset, s.id));
        }
        for (t, offset) in s.offsets {
            if !s.current.standby.contains(t) {
                candidates
                    .entry(t.clone())
                    .or_default()
                    .push((false, *offset, s.id));
            }
        }
    }
    for (t, mut c) in candidates {
        // Previous standbys first, then the most caught up; a stable sort keeps the members' order for the rest.
        c.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        state
            .prev_standby
            .insert(t, c.into_iter().map(|(_, _, m)| m).collect());
    }
    assign_active(&mut state, actives);
    if standbys > 0 {
        assign_standby(&mut state, stateful, standbys);
    }
    state.out
}

fn assign_active(state: &mut State<'_>, mut tasks: Vec<Task>) {
    tasks.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    // Where it runs now.
    tasks.retain(|t| {
        let Some(&m) = state.prev_active.get(t) else {
            return true;
        };
        if state.count(m) < state.active.each {
            state.add_active(m, t);
            return false;
        }
        true
    });
    // Where its state is.
    tasks.retain(|t| {
        let prev = state.prev_standby.get(t);
        match prev.and_then(|prev| state.least_loaded_of(prev, None)) {
            Some(m) if state.count(m) < state.active.each => {
                state.add_active(m, t);
                false
            }
            _ => true,
        }
    });
    // The rest, to the least loaded.
    tasks.sort();
    for t in &tasks {
        let Some(member) = state
            .lightest(&BTreeSet::new())
            .and_then(|p| state.member_in(p))
        else {
            return;
        };
        state.add_active(&member, t);
    }
}

fn assign_standby(state: &mut State<'_>, mut tasks: Vec<Task>, replicas: usize) {
    tasks.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    let mut left: Vec<(Task, usize)> = Vec::new();
    for t in tasks {
        for i in 0..replicas {
            if let Some(&m) = state.prev_active.get(&t)
                && state.process(m).is_some_and(|p| !p.has.contains(&t))
                && state.count(m) < state.total.each
            {
                state.add_standby(m, &t);
                continue;
            }
            let prev = state.prev_standby.get(&t);
            if let Some(m) = prev.and_then(|prev| state.least_loaded_of(prev, Some(&t)))
                && state.count(m) < state.total.each
            {
                state.add_standby(m, &t);
                continue;
            }
            left.push((t.clone(), replicas - i));
            break;
        }
    }
    left.sort_by(|a, b| b.0.cmp(&a.0));
    for (t, remaining) in left {
        for _ in 0..remaining {
            // The least loaded process that does not have the task; when none is left, the replica is not assigned
            // (Kafka logs that there is not enough capacity).
            let mut tried = BTreeSet::new();
            let target = loop {
                let Some(p) = state.lightest(&tried) else {
                    break None;
                };
                if state
                    .processes
                    .get(p)
                    .is_some_and(|ps| !ps.has.contains(&t))
                {
                    break Some(p);
                }
                tried.insert(p);
            };
            let Some(member) = target.and_then(|p| state.member_in(p)) else {
                break;
            };
            state.add_standby(&member, &t);
        }
    }
}
