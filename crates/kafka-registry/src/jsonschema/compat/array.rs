//! Arrays: items, tuples and what lies past them.

use super::*;

impl Diff<'_> {
    pub(super) fn array(&mut self, o: R, u: R, path: &str, out: &mut Out) {
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
    pub(super) fn additional(
        &mut self,
        (o, u): (R, R),
        path: &str,
        names: [&'static str; 4],
        out: &mut Out,
    ) {
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
pub(super) const ADDITIONAL_PROPERTIES: [&str; 4] = [
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
