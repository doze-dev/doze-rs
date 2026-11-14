//! JSON Schema compatibility, after Confluent's `SchemaDiff`.
//!
//! Confluent does not ask whether one schema's documents are a subset of the other's (undecidable in general); it
//! compares the two schemas structurally, keyword by keyword, and names each difference ("maxLength decreased",
//! "property added to an open content model"). A fixed set of those names is compatible. Where the rules look odd (a
//! property added to an open model breaks; `format` is ignored) they are Confluent's; the recorded corpus in
//! `tests/data/compat/json.json` is what this is held to.
//!
//! Every comparison of two nodes is asked once: whether it changes anything and whether it breaks anything is
//! remembered per pair, so shared subschemas (a diamond of references) and the pairing of combined schemas' branches
//! cost the pairs there are, not the paths to them. The reasons are then listed by one walk that only goes where
//! something breaks.

use std::collections::HashMap;

use super::node::{ANY, Arena, Arr, Crit, Id, Kind, Node, Obj};

/// The most reasons one comparison lists.
pub(crate) const MAX_REASONS: usize = 1000;

/// A difference that breaks a reader: Confluent's name for it and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) kind: &'static str,
    pub(crate) path: String,
}

use scalar::{enumeration, limit_change, number, string};
use words::breaks;
pub(crate) use words::describe;

/// The names of a limit's four changes.
struct Limit {
    keyword: &'static str,
    added: &'static str,
    removed: &'static str,
    increased: &'static str,
    decreased: &'static str,
}

macro_rules! limit {
    ($kw:literal, $p:literal) => {
        Limit {
            keyword: concat!("/", $kw),
            added: concat!($p, "_ADDED"),
            removed: concat!($p, "_REMOVED"),
            increased: concat!($p, "_INCREASED"),
            decreased: concat!($p, "_DECREASED"),
        }
    };
}

// After the macro, which they use.
mod array;
mod object;
mod scalar;
mod words;

/// A node of one of the two schemas: side 0 is the one compared from (the writer's), 1 the one compared to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct R(u8, Id);

impl R {
    fn to(self, id: Id) -> Self {
        Self(self.0, id)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Sum {
    changed: bool,
    bad: bool,
}

/// What a comparison gathers: in a question, only whether anything changed or broke; in the listing walk, also each
/// breaking change with its path.
struct Out {
    listing: bool,
    sum: Sum,
    list: Vec<Change>,
}

impl Out {
    fn add(&mut self, kind: &'static str, path: &str, seg: &str) {
        let bad = breaks(kind);
        self.sum.changed = true;
        self.sum.bad |= bad;
        if self.listing && bad && self.list.len() < MAX_REASONS {
            self.list.push(Change {
                kind,
                path: format!("{path}{seg}"),
            });
        }
    }

    fn at(&self, path: &str, seg: &str) -> String {
        if self.listing {
            format!("{path}{seg}")
        } else {
            String::new()
        }
    }
}

/// Every breaking change going from `old` (what was written) to `new` (what reads it), in Confluent's order.
pub(crate) fn changes(old: &Arena, new: &Arena) -> Vec<Change> {
    let mut d = Diff {
        arenas: [old, new],
        memo: HashMap::new(),
    };
    let (o, u) = (R(0, old.root()), R(1, new.root()));
    let mut out = Out {
        listing: true,
        sum: Sum::default(),
        list: Vec::new(),
    };
    if d.sum(o, u).bad {
        d.compare(o, u, "#", &mut out);
    }
    out.list
}

struct Diff<'a> {
    arenas: [&'a Arena; 2],
    memo: HashMap<(R, R), Sum>,
}

impl<'a> Diff<'a> {
    fn node(&self, r: R) -> &'a Node {
        let arena = if r.0 == 0 {
            self.arenas[0]
        } else {
            self.arenas[1]
        };
        arena.node(r.1)
    }

    /// Whether comparing `o` with `u` changes anything, and breaks anything (asked once per pair).
    fn sum(&mut self, o: R, u: R) -> Sum {
        if let Some(s) = self.memo.get(&(o, u)) {
            return *s;
        }
        let mut q = Out {
            listing: false,
            sum: Sum::default(),
            list: Vec::new(),
        };
        self.compare(o, u, "", &mut q);
        self.memo.insert((o, u), q.sum);
        q.sum
    }

    fn identical(&mut self, o: R, u: R) -> bool {
        !self.sum(o, u).changed
    }

    fn compatible(&mut self, o: R, u: R) -> bool {
        !self.sum(o, u).bad
    }

    /// The changes from `o` to `u` count as this comparison's own; the walk lists them where they break.
    fn child(&mut self, o: R, u: R, path: &str, seg: &str, out: &mut Out) {
        let s = self.sum(o, u);
        out.sum.changed |= s.changed;
        out.sum.bad |= s.bad;
        if out.listing && s.bad && out.list.len() < MAX_REASONS {
            let p = format!("{path}{seg}");
            self.compare(o, u, &p, out);
        }
    }

    fn compare(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let (on, un) = (&self.node(o).kind, &self.node(u).kind);
        let (o_any, u_any) = (matches!(on, Kind::Any), matches!(un, Kind::Any));
        let (o_none, u_none) = (matches!(on, Kind::Nothing), matches!(un, Kind::Nothing));
        if (o_any && u_any) || (o_none && u_none) {
            return;
        }
        if o_none || u_any {
            // Nothing was accepted before, or anything is now.
            return out.add("TYPE_EXTENDED", path, "");
        }
        if u_none || o_any {
            return out.add("TYPE_CHANGED", path, "");
        }
        match (on, un) {
            (Kind::Combined(..), Kind::Combined(..)) => self.combined(o, u, path, out),
            (Kind::Combined(..), _) => self.combined_to_single(o, u, path, out),
            (_, Kind::Combined(..)) => self.single_to_combined(o, u, path, out),
            _ => self.same_kind(o, u, path, out),
        }
    }

    /// A combination becoming a single schema: one branch is that schema; an allOf losing parts accepts more when what
    /// is left is one of them (recorded: "not removed").
    fn combined_to_single(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let Kind::Combined(crit, subs) = &self.node(o).kind else {
            return;
        };
        if let [one] = subs.as_slice() {
            return self.child(o.to(*one), u, path, "", out);
        }
        if *crit == Crit::All
            && let Some(sub) = subs
                .iter()
                .map(|s| o.to(*s))
                .find(|s| self.compatible(*s, u))
        {
            self.child(sub, u, path, "", out);
            return out.add("PRODUCT_TYPE_NARROWED", path, "");
        }
        out.add("TYPE_CHANGED", path, "");
    }

    /// A single schema becoming a combination: a choice that includes it accepts more (recorded: "type to type array
    /// superset"); parts added to it are a change of kind (recorded: "not added", "if then else added").
    fn single_to_combined(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let Kind::Combined(crit, subs) = &self.node(u).kind else {
            return;
        };
        if let [one] = subs.as_slice() {
            return self.child(o, u.to(*one), path, "", out);
        }
        if *crit != Crit::All
            && let Some(sub) = subs
                .iter()
                .map(|s| u.to(*s))
                .find(|s| self.compatible(o, *s))
        {
            self.child(o, sub, path, "", out);
            return out.add("SUM_TYPE_EXTENDED", path, "");
        }
        out.add("TYPE_CHANGED", path, "");
    }

    fn same_kind(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        match (&self.node(o).kind, &self.node(u).kind) {
            (Kind::Str(a), Kind::Str(b)) => string(a, b, path, out),
            (Kind::Num(a), Kind::Num(b)) => number(a, b, path, out),
            (Kind::Arr(_), Kind::Arr(_)) => self.array(o, u, path, out),
            (Kind::Obj(_), Kind::Obj(_)) => self.object(o, u, path, out),
            (Kind::Enum(a), Kind::Enum(b)) => enumeration(a, b, path, out),
            (Kind::Const(a), Kind::Const(b)) => {
                if a != b {
                    out.add("ENUM_ARRAY_CHANGED", path, "/const"); // recorded: "const changed"
                }
            }
            (Kind::Cond(a), Kind::Cond(b)) => {
                let changed = a.iter().zip(b).any(|(x, y)| match (x, y) {
                    (None, None) => false,
                    (Some(x), Some(y)) => {
                        let (x, y) = (o.to(*x), u.to(*y));
                        !(self.compatible(x, y) && self.compatible(y, x))
                    }
                    _ => true,
                });
                if changed {
                    out.add("CONDITIONAL_CHANGED", path, "");
                }
            }
            (Kind::Not(a), Kind::Not(b)) => {
                // not(A) to not(B) accepts more when B is narrower than A.
                let narrowed = self.compatible(u.to(*b), o.to(*a));
                out.add(
                    if narrowed {
                        "NOT_TYPE_NARROWED"
                    } else {
                        "NOT_TYPE_EXTENDED"
                    },
                    path,
                    "",
                );
            }
            (Kind::Bool, Kind::Bool) | (Kind::Null, Kind::Null) => {}
            _ => out.add("TYPE_CHANGED", path, ""),
        }
    }
}

impl Diff<'_> {
    /// Both sides combined (allOf, anyOf, oneOf). As Confluent's diff: a change in the number of branches is named by
    /// which side has more; then each old branch is paired with a new one, identical ones first, then compatible ones;
    /// if fewer pair up than the smaller side has, the branches changed; otherwise each pair is compared.
    fn combined(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let (Kind::Combined(oc, os), Kind::Combined(uc, us)) =
            (&self.node(o).kind, &self.node(u).kind)
        else {
            return;
        };
        let crit = match (*oc, *uc) {
            (a, b) if a == b => a,
            (Crit::One, Crit::Any) => Crit::Any,
            (Crit::Any, Crit::One) => return out.add("COMBINED_TYPE_CHANGED", path, ""),
            (_, b) if os.len() == 1 => b,
            (a, _) if us.len() == 1 => a,
            _ => return out.add("COMBINED_TYPE_CHANGED", path, ""),
        };
        let product = crit == Crit::All;
        match os.len().cmp(&us.len()) {
            std::cmp::Ordering::Less => out.add(
                if product {
                    "PRODUCT_TYPE_EXTENDED"
                } else {
                    "SUM_TYPE_EXTENDED"
                },
                path,
                "",
            ),
            std::cmp::Ordering::Greater => out.add(
                if product {
                    "PRODUCT_TYPE_NARROWED"
                } else {
                    "SUM_TYPE_NARROWED"
                },
                path,
                "",
            ),
            std::cmp::Ordering::Equal => {}
        }
        let os: Vec<R> = os.iter().map(|x| o.to(*x)).collect();
        let us: Vec<R> = us.iter().map(|x| u.to(*x)).collect();
        let pairs = self.pair(&os, &us);
        if pairs.len() < os.len().min(us.len()) {
            return out.add("COMBINED_TYPE_SUBSCHEMAS_CHANGED", path, "");
        }
        for (i, j) in pairs {
            if let (Some(x), Some(y)) = (os.get(i), us.get(j)) {
                self.child(*x, *y, path, &format!("/{}/{i}", crit.name()), out);
            }
        }
    }

    /// Pairs each old branch with a new one, identical ones first, then compatible ones; by old branch.
    fn pair(&mut self, os: &[R], us: &[R]) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        let (mut used_o, mut used_u) = (vec![false; os.len()], vec![false; us.len()]);
        for identical_first in [true, false] {
            for (i, ob) in os.iter().enumerate() {
                if used_o.get(i).copied().unwrap_or(true) {
                    continue;
                }
                let found = us.iter().enumerate().find_map(|(j, ub)| {
                    let free = !used_u.get(j).copied().unwrap_or(true);
                    let fits = free
                        && if identical_first {
                            self.identical(*ob, *ub)
                        } else {
                            self.compatible(*ob, *ub)
                        };
                    fits.then_some(j)
                });
                if let Some(j) = found {
                    mark(&mut used_o, i);
                    mark(&mut used_u, j);
                    pairs.push((i, j));
                }
            }
        }
        pairs.sort_unstable();
        pairs
    }
}

fn mark(used: &mut [bool], i: usize) {
    if let Some(u) = used.get_mut(i) {
        *u = true;
    }
}
