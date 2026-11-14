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

use super::node::{ANY, Arena, Arr, Crit, Id, Kind, Node, Num, Obj, Str};
use super::number::Dec;

/// The most reasons one comparison lists.
pub(crate) const MAX_REASONS: usize = 1000;

/// A difference that breaks a reader: Confluent's name for it and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) kind: &'static str,
    pub(crate) path: String,
}

/// The differences a reader can live with.
const COMPATIBLE: &[&str] = &[
    "TYPE_EXTENDED",
    "SCHEMA_REMOVED",
    "MAX_LENGTH_REMOVED",
    "MAX_LENGTH_INCREASED",
    "MIN_LENGTH_REMOVED",
    "MIN_LENGTH_DECREASED",
    "PATTERN_REMOVED",
    "MAXIMUM_REMOVED",
    "MAXIMUM_INCREASED",
    "MINIMUM_REMOVED",
    "MINIMUM_DECREASED",
    "EXCLUSIVE_MAXIMUM_REMOVED",
    "EXCLUSIVE_MAXIMUM_INCREASED",
    "EXCLUSIVE_MINIMUM_REMOVED",
    "EXCLUSIVE_MINIMUM_DECREASED",
    "MULTIPLE_OF_REMOVED",
    "MULTIPLE_OF_REDUCED",
    "REQUIRED_ATTRIBUTE_WITH_DEFAULT_ADDED",
    "REQUIRED_ATTRIBUTE_REMOVED",
    "DEPENDENCY_ARRAY_REMOVED",
    "DEPENDENCY_ARRAY_NARROWED",
    "PROPERTY_REMOVED_FROM_OPEN_CONTENT_MODEL",
    "PROPERTY_ADDED_TO_CLOSED_CONTENT_MODEL",
    "REQUIRED_PROPERTY_WITH_DEFAULT_ADDED_TO_UNOPEN_CONTENT_MODEL",
    "PROPERTY_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "PROPERTY_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ADDITIONAL_PROPERTIES_ADDED",
    "ADDITIONAL_PROPERTIES_EXTENDED",
    "MAX_PROPERTIES_REMOVED",
    "MAX_PROPERTIES_INCREASED",
    "MIN_PROPERTIES_REMOVED",
    "MIN_PROPERTIES_DECREASED",
    "MAX_ITEMS_REMOVED",
    "MAX_ITEMS_INCREASED",
    "MIN_ITEMS_REMOVED",
    "MIN_ITEMS_DECREASED",
    "UNIQUE_ITEMS_REMOVED",
    "ADDITIONAL_ITEMS_ADDED",
    "ADDITIONAL_ITEMS_EXTENDED",
    "ITEM_REMOVED_FROM_CLOSED_CONTENT_MODEL",
    "ITEM_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ITEM_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
    "ITEM_REMOVED_FROM_OPEN_CONTENT_MODEL",
    "ITEM_ADDED_TO_CLOSED_CONTENT_MODEL",
    "ENUM_ARRAY_EXTENDED",
    "COMBINED_TYPE_EXTENDED",
    "PRODUCT_TYPE_NARROWED",
    "SUM_TYPE_EXTENDED",
    "NOT_TYPE_NARROWED",
];

fn breaks(kind: &str) -> bool {
    !COMPATIBLE.contains(&kind)
}

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

fn limit_change(l: &Limit, o: Option<&Dec>, u: Option<&Dec>, path: &str, out: &mut Out) {
    let kind = match (o, u) {
        (None, None) => return,
        (None, Some(_)) => l.added,
        (Some(_), None) => l.removed,
        (Some(o), Some(u)) => match o.cmp(u) {
            std::cmp::Ordering::Less => l.increased,
            std::cmp::Ordering::Greater => l.decreased,
            std::cmp::Ordering::Equal => return,
        },
    };
    out.add(kind, path, l.keyword);
}

fn string(o: &Str, u: &Str, path: &str, out: &mut Out) {
    limit_change(
        &limit!("maxLength", "MAX_LENGTH"),
        o.max_len.as_ref(),
        u.max_len.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("minLength", "MIN_LENGTH"),
        o.min_len.as_ref(),
        u.min_len.as_ref(),
        path,
        out,
    );
    match (&o.pattern, &u.pattern) {
        (None, Some(_)) => out.add("PATTERN_ADDED", path, "/pattern"),
        (Some(_), None) => out.add("PATTERN_REMOVED", path, "/pattern"),
        (Some(a), Some(b)) if a != b => out.add("PATTERN_CHANGED", path, "/pattern"),
        _ => {}
    }
}

fn number(o: &Num, u: &Num, path: &str, out: &mut Out) {
    // The limits first, then whether it became integer only (Confluent's order, recorded: "random 114").
    limit_change(
        &limit!("maximum", "MAXIMUM"),
        o.max.as_ref(),
        u.max.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("minimum", "MINIMUM"),
        o.min.as_ref(),
        u.min.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("exclusiveMaximum", "EXCLUSIVE_MAXIMUM"),
        o.ex_max.as_ref(),
        u.ex_max.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("exclusiveMinimum", "EXCLUSIVE_MINIMUM"),
        o.ex_min.as_ref(),
        u.ex_min.as_ref(),
        path,
        out,
    );
    match (&o.multiple_of, &u.multiple_of) {
        (None, Some(_)) => out.add("MULTIPLE_OF_ADDED", path, "/multipleOf"),
        (Some(_), None) => out.add("MULTIPLE_OF_REMOVED", path, "/multipleOf"),
        (Some(a), Some(b)) if a != b => {
            let kind = if b.is_multiple_of(a) {
                "MULTIPLE_OF_EXPANDED"
            } else if a.is_multiple_of(b) {
                "MULTIPLE_OF_REDUCED"
            } else {
                "MULTIPLE_OF_CHANGED"
            };
            out.add(kind, path, "/multipleOf");
        }
        _ => {}
    }
    match (o.integer, u.integer) {
        (true, false) => out.add("TYPE_EXTENDED", path, ""),
        (false, true) => out.add("TYPE_NARROWED", path, ""),
        _ => {}
    }
}

fn enumeration(o: &[String], u: &[String], path: &str, out: &mut Out) {
    let o_in_u = o.iter().all(|v| u.contains(v));
    let u_in_o = u.iter().all(|v| o.contains(v));
    let kind = match (o_in_u, u_in_o) {
        (true, true) => return,
        (true, false) => "ENUM_ARRAY_EXTENDED",
        (false, true) => "ENUM_ARRAY_NARROWED",
        (false, false) => "ENUM_ARRAY_CHANGED",
    };
    out.add(kind, path, "/enum");
}

impl Diff<'_> {
    fn array(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let (Kind::Arr(a), Kind::Arr(b)) = (&self.node(o).kind, &self.node(u).kind) else {
            return;
        };
        limit_change(
            &limit!("maxItems", "MAX_ITEMS"),
            a.max_items.as_ref(),
            b.max_items.as_ref(),
            path,
            out,
        );
        limit_change(
            &limit!("minItems", "MIN_ITEMS"),
            a.min_items.as_ref(),
            b.min_items.as_ref(),
            path,
            out,
        );
        match (a.unique, b.unique) {
            (false, true) => out.add("UNIQUE_ITEMS_ADDED", path, "/uniqueItems"),
            (true, false) => out.add("UNIQUE_ITEMS_REMOVED", path, "/uniqueItems"),
            _ => {}
        }
        if a.tuple.is_none() && b.tuple.is_none() {
            // An items schema on one side only is a schema added or removed, as Confluent names it.
            return match (a.items, b.items) {
                (None, None) => {}
                (None, Some(_)) => out.add("SCHEMA_ADDED", path, "/items"),
                (Some(_), None) => out.add("SCHEMA_REMOVED", path, "/items"),
                (Some(x), Some(y)) => self.child(o.to(x), u.to(y), path, "/items", out),
            };
        }
        let count = a
            .tuple
            .as_ref()
            .map_or(0, Vec::len)
            .max(b.tuple.as_ref().map_or(0, Vec::len));
        for i in 0..count {
            let seg = format!("/items/{i}");
            match (position(a, i), position(b, i)) {
                ((x, true), (y, true)) => self.child(o.to(x), u.to(y), path, &seg, out),
                ((_, false), (y, true)) => {
                    let kind = self.item_moved(a.additional.map(|x| o.to(x)), u.to(y), true);
                    out.add(kind, path, &seg);
                }
                ((x, true), (_, false)) => {
                    let kind = self.item_moved(b.additional.map(|y| u.to(y)), o.to(x), false);
                    out.add(kind, path, &seg);
                }
                _ => {}
            }
        }
        match (a.additional, b.additional) {
            (None, Some(_)) => out.add("ADDITIONAL_ITEMS_REMOVED", path, "/additionalItems"),
            (Some(_), None) => out.add("ADDITIONAL_ITEMS_ADDED", path, "/additionalItems"),
            (Some(x), Some(y)) => {
                let p = out.at(path, "/additionalItems");
                self.additional((o.to(x), u.to(y)), &p, ADDITIONAL_ITEMS, out);
            }
            (None, None) => {}
        }
    }

    /// A position one schema describes and the other leaves to its additionalItems (`rest`).
    fn item_moved(&mut self, rest: Option<R>, item: R, added: bool) -> &'static str {
        let Some(rest) = rest else {
            return if added {
                "ITEM_ADDED_TO_OPEN_CONTENT_MODEL"
            } else {
                "ITEM_REMOVED_FROM_OPEN_CONTENT_MODEL"
            };
        };
        if matches!(self.node(rest).kind, Kind::Nothing) {
            return if added {
                "ITEM_ADDED_TO_CLOSED_CONTENT_MODEL"
            } else {
                "ITEM_REMOVED_FROM_CLOSED_CONTENT_MODEL"
            };
        }
        let covered = self.compatible(rest, item) && self.compatible(item, rest);
        match (added, covered) {
            (true, true) => "ITEM_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
            (true, false) => "ITEM_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
            (false, true) => "ITEM_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
            (false, false) => "ITEM_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
        }
    }

    /// Two additionalProperties or additionalItems (at `path`) that are each `false` or a schema.
    fn additional(&mut self, (o, u): (R, R), path: &str, names: [&'static str; 4], out: &mut Out) {
        let [added, removed, extended, narrowed] = names;
        match (
            matches!(self.node(o).kind, Kind::Nothing),
            matches!(self.node(u).kind, Kind::Nothing),
        ) {
            (true, true) => {}
            (true, false) => out.add(added, path, ""),
            (false, true) => out.add(removed, path, ""),
            (false, false) => {
                let s = self.sum(o, u);
                if s.bad {
                    out.add(narrowed, path, "");
                } else if s.changed {
                    out.add(extended, path, "");
                }
            }
        }
    }
}

const ADDITIONAL_ITEMS: [&str; 4] = [
    "ADDITIONAL_ITEMS_ADDED",
    "ADDITIONAL_ITEMS_REMOVED",
    "ADDITIONAL_ITEMS_EXTENDED",
    "ADDITIONAL_ITEMS_NARROWED",
];
const ADDITIONAL_PROPERTIES: [&str; 4] = [
    "ADDITIONAL_PROPERTIES_ADDED",
    "ADDITIONAL_PROPERTIES_REMOVED",
    "ADDITIONAL_PROPERTIES_EXTENDED",
    "ADDITIONAL_PROPERTIES_NARROWED",
];

/// The schema for position `i` of an array, and whether the array describes it (rather than leaving it to
/// additionalItems).
fn position(a: &Arr, i: usize) -> (Id, bool) {
    match &a.tuple {
        None => a.items.map_or((ANY, false), |x| (x, true)),
        Some(t) => match t.get(i) {
            Some(x) => (*x, true),
            None => (a.additional.unwrap_or(ANY), false),
        },
    }
}

impl Diff<'_> {
    fn object(&mut self, o: R, u: R, path: &str, out: &mut Out) {
        let (Kind::Obj(a), Kind::Obj(b)) = (&self.node(o).kind, &self.node(u).kind) else {
            return;
        };
        // In Confluent's order (recorded: json-reasons.json): required, then each property, then additionalProperties,
        // then the counts, then dependencies.
        self.required(u, (a, b), path, out);
        let o_names: Vec<String> = a.props.iter().map(|(n, _)| n.clone()).collect();
        let u_only: Vec<String> = b
            .props
            .iter()
            .filter(|(n, _)| a.prop(n).is_none())
            .map(|(n, _)| n.clone())
            .collect();
        // The compared-from schema's names first: two names in one bucket keep that order (recorded: "random 003").
        let mut names = java_hash_order(o_names);
        names.extend(java_hash_order(u_only));
        for name in java_hash_inserted(names) {
            let seg = format!("/properties/{name}");
            let (oa, ua) = (a.additional.map(|x| o.to(x)), b.additional.map(|y| u.to(y)));
            match (a.prop(&name), b.prop(&name)) {
                (Some(x), Some(y)) => self.child(o.to(x), u.to(y), path, &seg, out),
                (None, Some(y)) => {
                    self.property_added(oa, (u.to(y), b.requires(&name)), (path, &seg), out)
                }
                (Some(x), None) => self.property_removed(o.to(x), ua, (path, &seg), out),
                (None, None) => {}
            }
        }
        self.additional_properties((o, u), (a.additional, b.additional), path, out);
        limit_change(
            &limit!("maxProperties", "MAX_PROPERTIES"),
            a.max_props.as_ref(),
            b.max_props.as_ref(),
            path,
            out,
        );
        limit_change(
            &limit!("minProperties", "MIN_PROPERTIES"),
            a.min_props.as_ref(),
            b.min_props.as_ref(),
            path,
            out,
        );
        dependencies(a, b, path, out);
    }

    fn additional_properties(
        &mut self,
        (o, u): (R, R),
        extra: (Option<Id>, Option<Id>),
        path: &str,
        out: &mut Out,
    ) {
        const AT: &str = "/additionalProperties";
        match extra {
            (None, None) => {}
            (None, Some(y)) => {
                let closed = matches!(self.node(u.to(y)).kind, Kind::Nothing);
                out.add(
                    if closed {
                        "ADDITIONAL_PROPERTIES_REMOVED"
                    } else {
                        "ADDITIONAL_PROPERTIES_NARROWED"
                    },
                    path,
                    AT,
                );
            }
            (Some(x), None) => {
                let closed = matches!(self.node(o.to(x)).kind, Kind::Nothing);
                out.add(
                    if closed {
                        "ADDITIONAL_PROPERTIES_ADDED"
                    } else {
                        "ADDITIONAL_PROPERTIES_EXTENDED"
                    },
                    path,
                    AT,
                );
            }
            (Some(x), Some(y)) => {
                let (x, y) = (o.to(x), u.to(y));
                if matches!(self.node(x).kind, Kind::Nothing)
                    || matches!(self.node(y).kind, Kind::Nothing)
                {
                    let p = out.at(path, AT);
                    self.additional((x, y), &p, ADDITIONAL_PROPERTIES, out);
                } else {
                    // Both allow more properties of a schema: what changed in it is the change (recorded: "partial:
                    // widen additional type").
                    self.child(x, y, path, AT, out);
                }
            }
        }
    }

    /// A property both declare that is now required narrows what is accepted, unless it has a default; one the new
    /// schema adds is judged with the property.
    fn required(&self, u: R, (a, b): (&Obj, &Obj), path: &str, out: &mut Out) {
        let added: Vec<String> = b
            .required
            .iter()
            .filter(|r| !a.requires(r))
            .cloned()
            .collect();
        let removed: Vec<String> = a
            .required
            .iter()
            .filter(|r| !b.requires(r))
            .cloned()
            .collect();
        for r in java_hash_order(added) {
            if let (Some(_), Some(y)) = (a.prop(&r), b.prop(&r)) {
                let kind = if self.node(u.to(y)).has_default {
                    "REQUIRED_ATTRIBUTE_WITH_DEFAULT_ADDED"
                } else {
                    "REQUIRED_ATTRIBUTE_ADDED"
                };
                out.add(kind, path, &format!("/required/{r}"));
            }
        }
        for r in java_hash_order(removed) {
            out.add(
                "REQUIRED_ATTRIBUTE_REMOVED",
                path,
                &format!("/required/{r}"),
            );
        }
    }

    /// A property the old schema did not name, judged by the old schema's additionalProperties (`extra`); `up` is the
    /// new property and whether the new schema requires it.
    fn property_added(
        &mut self,
        extra: Option<R>,
        (up, required): (R, bool),
        (path, seg): (&str, &str),
        out: &mut Out,
    ) {
        let Some(extra) = extra else {
            return out.add("PROPERTY_ADDED_TO_OPEN_CONTENT_MODEL", path, seg);
        };
        if matches!(self.node(extra).kind, Kind::Nothing) {
            out.add("PROPERTY_ADDED_TO_CLOSED_CONTENT_MODEL", path, seg);
        } else if self.compatible(extra, up) {
            out.add(
                "PROPERTY_ADDED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
                path,
                seg,
            );
        } else {
            // Why it is not covered first, as Confluent lists it.
            let p = out.at(path, seg);
            self.child(extra, up, &p, "", out);
            out.add(
                "PROPERTY_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
                path,
                seg,
            );
        }
        // Required in a model that was not open: data written before it has none.
        if required {
            let kind = if self.node(up).has_default {
                "REQUIRED_PROPERTY_WITH_DEFAULT_ADDED_TO_UNOPEN_CONTENT_MODEL"
            } else {
                "REQUIRED_PROPERTY_ADDED_TO_UNOPEN_CONTENT_MODEL"
            };
            out.add(kind, path, seg);
        }
    }

    /// A property the new schema does not name, judged by the new schema's additionalProperties (`extra`).
    fn property_removed(
        &mut self,
        op: R,
        extra: Option<R>,
        (path, seg): (&str, &str),
        out: &mut Out,
    ) {
        let Some(extra) = extra else {
            return out.add("PROPERTY_REMOVED_FROM_OPEN_CONTENT_MODEL", path, seg);
        };
        if matches!(self.node(extra).kind, Kind::Nothing) {
            out.add("PROPERTY_REMOVED_FROM_CLOSED_CONTENT_MODEL", path, seg);
        } else if self.compatible(op, extra) {
            out.add(
                "PROPERTY_REMOVED_IS_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
                path,
                seg,
            );
        } else {
            let p = out.at(path, seg);
            self.child(op, extra, &p, "", out);
            out.add(
                "PROPERTY_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL",
                path,
                seg,
            );
        }
    }
}

/// Dependencies: a property that now depends on more is a narrowing.
fn dependencies(a: &Obj, b: &Obj, path: &str, out: &mut Out) {
    let of = |o: &Obj, k: &str| {
        o.dependencies
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
    };
    for k in java_hash_order(b.dependencies.iter().map(|(k, _)| k.clone()).collect()) {
        let seg = format!("/dependencies/{k}");
        let uv = of(b, &k).unwrap_or_default();
        match of(a, &k) {
            None => out.add("DEPENDENCY_ARRAY_ADDED", path, &seg),
            Some(ov) if ov != uv => {
                let narrowed = uv.iter().all(|x| ov.contains(x));
                out.add(
                    if narrowed {
                        "DEPENDENCY_ARRAY_NARROWED"
                    } else {
                        "DEPENDENCY_ARRAY_EXTENDED"
                    },
                    path,
                    &seg,
                );
            }
            Some(_) => {}
        }
    }
    let gone = a
        .dependencies
        .iter()
        .filter(|(k, _)| of(b, k).is_none())
        .map(|(k, _)| k.clone())
        .collect();
    for k in java_hash_order(gone) {
        out.add(
            "DEPENDENCY_ARRAY_REMOVED",
            path,
            &format!("/dependencies/{k}"),
        );
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

/// The order a `java.util.HashMap` built from these names iterates them in: by bucket (`String.hashCode`, spread,
/// masked by a capacity that grows from 16 at three quarters full), and within a bucket in the order they were put in,
/// taken here to be by name.
fn java_hash_order(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    java_hash_inserted(names)
}

/// [`java_hash_order`] for names inserted in the order given.
fn java_hash_inserted(mut names: Vec<String>) -> Vec<String> {
    let mut capacity: u32 = 16;
    while names.len() > (capacity as usize) * 3 / 4 {
        capacity = capacity.saturating_mul(2);
    }
    let bucket = |s: &String| {
        let h = s
            .encode_utf16()
            .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
        let u = h.cast_unsigned();
        (u ^ (u >> 16)) & (capacity - 1)
    };
    names.sort_by_key(bucket);
    names
}

/// Confluent's sentence for a change, the reader's side and the writer's named ("new" or "old"). A type the corpus has
/// not shown takes its family's sentence (all the *_ADDED, *_INCREASED ... that were recorded share one each).
pub(crate) fn describe(kind: &str, path: &str, reader: &str, writer: &str) -> String {
    const TYPE_DIFFERS: &str =
        "A type at path '%s' is different between the R schema and the W schema";
    const VALUE_DIFFERS: &str = "The value at path '%s' is different between the R and W schema";
    let t = match kind {
        "TYPE_CHANGED"
        | "TYPE_NARROWED"
        | "COMBINED_TYPE_CHANGED"
        | "COMBINED_TYPE_SUBSCHEMAS_CHANGED"
        | "ENUM_ARRAY_CHANGED" => TYPE_DIFFERS,
        "MULTIPLE_OF_CHANGED" | "PATTERN_CHANGED" => VALUE_DIFFERS,
        "ITEM_ADDED_TO_OPEN_CONTENT_MODEL" | "PROPERTY_ADDED_TO_OPEN_CONTENT_MODEL" => {
            "The R schema has an open content model and has a property or item at path '%s' which is missing in the W schema"
        }
        "PROPERTY_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL"
        | "ITEM_ADDED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL" => {
            "The R schema has a property or item at path '%s' which is missing in the W schema and is not covered by its partially open content model"
        }
        "PROPERTY_REMOVED_FROM_CLOSED_CONTENT_MODEL" | "ITEM_REMOVED_FROM_CLOSED_CONTENT_MODEL" => {
            "The R has a closed content model and is missing a property or item present at path '%s' in the W schema"
        }
        "PROPERTY_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL"
        | "ITEM_REMOVED_NOT_COVERED_BY_PARTIALLY_OPEN_CONTENT_MODEL" => {
            "A property or item is missing in the R schema but present at path '%s' in the W schema and is not covered by its partially open content model"
        }
        "REQUIRED_PROPERTY_ADDED_TO_UNOPEN_CONTENT_MODEL" => {
            "The R schema has an unopen content model and has a required property at path '%s' which is missing in the W schema"
        }
        "SCHEMA_ADDED" => return String::new(),
        k if k.ends_with("_ADDED") || k.ends_with("_REMOVED") => {
            "The keyword at path '%s' in the R schema is not present in the W schema"
        }
        k if k.ends_with("_INCREASED") || k.ends_with("_EXPANDED") => {
            "The value at path '%s' in the R schema is more than its value in the W schema"
        }
        k if k.ends_with("_DECREASED") || k.ends_with("_REDUCED") => {
            "The value at path '%s' in the R schema is less than its value in the W schema"
        }
        k if k.ends_with("_NARROWED") => {
            "An array or combined type at path '%s' has fewer elements in the R schema than the W schema"
        }
        k if k.ends_with("_EXTENDED") => {
            "An array or combined type at path '%s' has more elements in the R schema than the W schema"
        }
        _ => TYPE_DIFFERS,
    };
    let path = if path == "#" { "#/" } else { path }; // the root, as Confluent writes it
    t.replace("R and W", &format!("{reader} and {writer}"))
        .replace("The R has", &format!("The {reader} has"))
        .replace("R schema", &format!("{reader} schema"))
        .replace("W schema", &format!("{writer} schema"))
        .replace("%s", path)
}
