//! Objects: required, properties in open, closed and partially open content models, additionalProperties, dependencies.

use super::array::ADDITIONAL_PROPERTIES;
use super::scalar::{java_hash_inserted, java_hash_order, limit_change};
use super::*;

impl Diff<'_> {
    pub(super) fn object(&mut self, o: R, u: R, path: &str, out: &mut Out) {
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
