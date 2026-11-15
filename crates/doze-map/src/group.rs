//! Groups, and collapsing a group into one card. A group is what the caller says (an instance's
//! `group`), or what doze.hcl's `count` and `for_each` stamp: `worker_0`, `worker_1` … are
//! `label_suffix`, one block's instances. A collapsed group is one card (`worker_*`) that stands for
//! all of them: its calls and uses are theirs together, and a call between two groups counts the
//! calls it stands for. Collapsing happens before layout, so a stack of 2,000 instances stamped by
//! forty blocks lays out as forty cards.

use std::collections::{HashMap, HashSet};

use crate::input::{Instance, Kind};

/// Where groups come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Grouping {
    /// No groups.
    None,
    /// Only [`Instance::group`].
    Explicit,
    /// Only the `label_suffix` names `count` and `for_each` stamp.
    Stamps,
    /// [`Instance::group`] where set, else the stamp.
    #[default]
    Both,
}

/// When groups collapse into one card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collapse {
    /// A group needs at least this many instances to collapse.
    pub min_members: usize,
    /// Collapse every group when the stack has more instances than this.
    pub over: usize,
    /// Groups the user opened: never collapsed.
    pub expanded: HashSet<String>,
    /// Groups the user closed: always collapsed.
    pub collapsed: HashSet<String>,
}

impl Default for Collapse {
    fn default() -> Self {
        Collapse {
            min_members: 3,
            over: 400,
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
        }
    }
}

/// The stack with its collapsed groups as one instance each.
#[derive(Debug, Clone, Default)]
pub(crate) struct Collapsed {
    pub(crate) ins: Vec<Instance>,
    /// A group card's name, then its members' names.
    pub(crate) groups: HashMap<String, Vec<String>>,
    /// How many calls each call between cards stands for, where more than one.
    pub(crate) counts: HashMap<(String, String), u32>,
}

/// The label an instance is grouped under, and the card name its group collapses to.
fn label(v: &Instance, how: Grouping) -> Option<(String, String)> {
    if v.kind == Kind::Task {
        return None;
    }
    let explicit = || {
        v.group
            .as_ref()
            .filter(|g| !g.is_empty())
            .map(|g| (g.clone(), format!("{g}/*")))
    };
    let stamp = || {
        v.name
            .rsplit_once('_')
            .filter(|(l, s)| !l.is_empty() && !s.is_empty())
            .map(|(l, _)| (l.to_string(), format!("{l}_*")))
    };
    match how {
        Grouping::None => None,
        Grouping::Explicit => explicit(),
        Grouping::Stamps => stamp(),
        Grouping::Both => explicit().or_else(stamp),
    }
}

/// Collapses the groups `c` says to; `None` when nothing collapses.
pub(crate) fn collapse(ins: &[Instance], how: Grouping, c: &Collapse) -> Option<Collapsed> {
    let mut members: HashMap<(String, Kind), Vec<usize>> = HashMap::new();
    let mut card_of: Vec<Option<(String, String, Kind)>> = Vec::with_capacity(ins.len());
    for (i, v) in ins.iter().enumerate() {
        let l = label(v, how);
        if let Some((label, _)) = &l {
            members.entry((label.clone(), v.kind)).or_default().push(i);
        }
        card_of.push(l.map(|(label, card)| (label, card, v.kind)));
    }
    let big = ins.len() > c.over;
    let shut = |label: &str, n: usize| {
        n >= c.min_members.max(2)
            && !c.expanded.contains(label)
            && (big || c.collapsed.contains(label))
    };
    let rep: Vec<Option<String>> = card_of
        .iter()
        .map(|g| {
            let (label, card, kind) = g.as_ref()?;
            let n = members.get(&(label.clone(), *kind)).map_or(0, Vec::len);
            shut(label, n).then(|| card.clone())
        })
        .collect();
    if rep.iter().all(Option::is_none) {
        return None;
    }
    Some(build(ins, &rep))
}

fn build(ins: &[Instance], rep: &[Option<String>]) -> Collapsed {
    let name_rep: HashMap<&str, &str> = ins
        .iter()
        .zip(rep)
        .filter_map(|(v, r)| r.as_deref().map(|r| (v.name.as_str(), r)))
        .collect();
    let to = |n: &str| name_rep.get(n).copied().unwrap_or(n).to_string();
    let mut out = Collapsed::default();
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for (v, r) in ins.iter().zip(rep) {
        let me = r.clone().unwrap_or_else(|| v.name.clone());
        let k = match at.get(&me) {
            Some(&k) => k,
            None => {
                let mut g = v.clone();
                g.name.clone_from(&me);
                g.needs.clear();
                g.members = 0;
                out.ins.push(g);
                at.insert(me.clone(), out.ins.len() - 1);
                out.ins.len() - 1
            }
        };
        if r.is_some() {
            out.groups
                .entry(me.clone())
                .or_default()
                .push(v.name.clone());
        }
        let Some(g) = out.ins.get_mut(k) else {
            continue;
        };
        g.members += v.members.max(1);
        for need in &v.needs {
            let n = to(need);
            if n == me {
                continue;
            }
            if !g.needs.contains(&n) {
                g.needs.push(n.clone());
            }
            if seen.insert((v.name.clone(), need.clone())) {
                *out.counts.entry((me.clone(), n)).or_insert(0) += 1;
            }
        }
    }
    out.counts.retain(|_, n| *n > 1);
    out
}
