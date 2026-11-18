//! Tasks: a subtopology and a partition. A member holds them in three roles (active, standby, warm-up).

use std::collections::{BTreeMap, BTreeSet};

/// One task: its subtopology's id and its partition.
pub(crate) type Task = (String, i32);

/// Tasks by role.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Tasks {
    pub(crate) active: BTreeSet<Task>,
    pub(crate) standby: BTreeSet<Task>,
    pub(crate) warmup: BTreeSet<Task>,
}

impl Tasks {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.active.is_empty() && self.standby.is_empty() && self.warmup.is_empty()
    }

    /// Whether any task of `other` is held here in the same role.
    #[must_use]
    pub(crate) fn any_of(&self, other: &Self) -> bool {
        !self.active.is_disjoint(&other.active)
            || !self.standby.is_disjoint(&other.standby)
            || !self.warmup.is_disjoint(&other.warmup)
    }

    /// Whether every task here is in `other` in the same role.
    #[must_use]
    pub(crate) fn within(&self, other: &Self) -> bool {
        self.active.is_subset(&other.active)
            && self.standby.is_subset(&other.standby)
            && self.warmup.is_subset(&other.warmup)
    }

    /// Every task, whatever its role.
    pub(crate) fn all(&self) -> impl Iterator<Item = &Task> {
        self.active.iter().chain(&self.standby).chain(&self.warmup)
    }
}

/// Tasks grouped by subtopology, partitions in order: the wire's form.
#[must_use]
pub(crate) fn by_subtopology(tasks: &BTreeSet<Task>) -> BTreeMap<&str, Vec<i32>> {
    let mut out: BTreeMap<&str, Vec<i32>> = BTreeMap::new();
    for (sub, p) in tasks {
        out.entry(sub.as_str()).or_default().push(*p);
    }
    out
}

/// Tasks from the wire's form.
pub(crate) fn from_wire<'a>(
    lists: impl IntoIterator<Item = (&'a str, &'a [i32])>,
) -> BTreeSet<Task> {
    lists
        .into_iter()
        .flat_map(|(sub, ps)| ps.iter().map(move |p| (sub.to_owned(), *p)))
        .collect()
}
