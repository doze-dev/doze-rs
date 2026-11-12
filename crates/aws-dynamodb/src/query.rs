//! Query and Scan: what a request asks, turned into a plan (where to read, in which order, what to keep).

use aws_expr::{Cond, Kind, Path, Range, Scope, parse_condition, parse_key_condition};
use aws_wire::ApiError;
use serde_json::Value as Json;

use crate::condition::{item_member, projection, used};
use crate::core::{Core, Cx};
use crate::keys::{self, Space};
use crate::meta::{Index, Key, Projection, Table};
use crate::req::{Exprs, bad_param, bool_of, finish, int_of, inv, str_of, validation};
use crate::stage::{index_entry, index_prefix, pk_bytes};

/// Which attributes a read returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Select {
    All,
    Projected,
    Specific,
    Count,
}

/// A read, planned.
#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) t: Table,
    pub(crate) ix: Option<Index>,
    /// The sort key the read follows (the index's, or the table's).
    pub(crate) sort: Option<Key>,
    pub(crate) filter: Option<Cond>,
    pub(crate) proj: Option<Vec<Path>>,
    pub(crate) select: Select,
    pub(crate) limit: usize,
    pub(crate) forward: bool,
    pub(crate) space: Space,
    pub(crate) lo: Vec<u8>,
    pub(crate) hi: Vec<u8>,
    pub(crate) range: Option<Range>,
    pub(crate) segment: Option<(u32, u32)>,
}

impl Plan {
    /// The key attributes of a position in the read: the table's, and the index's when there is one.
    pub(crate) fn position_keys(&self) -> Vec<&Key> {
        let mut keys: Vec<&Key> = std::iter::once(&self.t.hash)
            .chain(self.t.range.as_ref())
            .collect();
        if let Some(ix) = &self.ix {
            for k in std::iter::once(&ix.hash).chain(ix.range.as_ref()) {
                if !keys.iter().any(|x| x.name == k.name) {
                    keys.push(k);
                }
            }
        }
        keys
    }
}

fn select_of(
    body: &Json,
    ix: Option<&Index>,
    global: bool,
    has_projection: bool,
) -> Result<Select, ApiError> {
    let given = str_of(body, "Select");
    let select = match given {
        Some("ALL_ATTRIBUTES") => Select::All,
        Some("ALL_PROJECTED_ATTRIBUTES") => Select::Projected,
        Some("SPECIFIC_ATTRIBUTES") => Select::Specific,
        Some("COUNT") => Select::Count,
        _ if has_projection => Select::Specific,
        _ if global => Select::Projected,
        _ => Select::All,
    };
    if has_projection && given.is_some() && select != Select::Specific {
        return Err(bad_param(
            "Cannot specify ProjectionExpression with Select other than SPECIFIC_ATTRIBUTES",
        ));
    }
    if select == Select::Specific && !has_projection {
        return Err(bad_param(
            "Must specify the ProjectionExpression when choosing to get SPECIFIC_ATTRIBUTES",
        ));
    }
    match (select, ix) {
        (Select::Projected, None) => Err(bad_param(
            "ALL_PROJECTED_ATTRIBUTES can be used only when Querying using an IndexName",
        )),
        (Select::All, Some(i)) if global && i.projection != Projection::All => {
            Err(bad_param(&format!(
                "Select type ALL_ATTRIBUTES is not supported for global secondary index {} because its projection type is not ALL",
                i.name
            )))
        }
        _ => Ok(select),
    }
}

/// Whether `ix` is one of the table's global indexes.
fn is_global(t: &Table, ix: Option<&Index>) -> bool {
    ix.is_some_and(|i| t.gsis.iter().any(|g| g.name == i.name))
}

impl Core {
    /// Plans a Query.
    pub(crate) fn plan_query(&self, cx: &Cx<'_>, t: Table) -> Result<Plan, ApiError> {
        let ix = self.choose_index(cx, &t)?;
        let (hash, sort) = match &ix {
            Some(i) => (i.hash.clone(), i.range.clone()),
            None => (t.hash.clone(), t.range.clone()),
        };
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let text = str_of(&cx.body, "KeyConditionExpression").ok_or_else(|| {
            validation("Either the KeyConditions or KeyConditionExpression parameter must be specified in the request.")
        })?;
        let hash_ty = hash.ty.to_string();
        let sort_ty = sort.as_ref().map(|s| s.ty.to_string());
        let range = sort
            .as_ref()
            .zip(sort_ty.as_deref())
            .map(|(s, ty)| (s.name.as_str(), ty));
        let kc =
            parse_key_condition(text, &mut scope, (&hash.name, &hash_ty), range).map_err(inv)?;
        let (filter, proj) = self.filter_and_projection(cx, &mut scope)?;
        let labels = used(
            &cx.body,
            &[
                "KeyConditionExpression",
                "FilterExpression",
                "ProjectionExpression",
            ],
        );
        finish(&scope, &labels, "KeyConditionExpression")?;
        forbid_key_filter(
            filter.as_ref(),
            [Some(&hash), sort.as_ref(), Some(&t.hash), t.range.as_ref()],
        )?;
        let select = select_of(
            &cx.body,
            ix.as_ref(),
            is_global(&t, ix.as_ref()),
            proj.is_some(),
        )?;
        let mut prefix = ix
            .as_ref()
            .map_or_else(|| t.id.to_vec(), |i| index_prefix(&t, i));
        keys::encode(&kc.hash, &mut prefix);
        let forward = bool_of(&cx.body, "ScanIndexForward").unwrap_or(true);
        let (lo, hi) = range_bounds(&prefix, kc.range.as_ref(), forward);
        let mut plan = Plan {
            space: if ix.is_some() {
                Space::Index
            } else {
                Space::Items
            },
            t,
            ix,
            sort,
            filter,
            proj,
            select,
            limit: limit_of(&cx.body),
            forward,
            lo,
            hi,
            range: kc.range,
            segment: None,
        };
        self.start_after(cx, &mut plan, Some(&prefix))?;
        Ok(plan)
    }

    /// Plans a Scan.
    pub(crate) fn plan_scan(&self, cx: &Cx<'_>, t: Table) -> Result<Plan, ApiError> {
        let ix = self.choose_index(cx, &t)?;
        let ex = Exprs::of(&cx.body)?;
        let mut scope = ex.scope();
        let (filter, proj) = self.filter_and_projection(cx, &mut scope)?;
        finish(
            &scope,
            &used(&cx.body, &["FilterExpression", "ProjectionExpression"]),
            "FilterExpression",
        )?;
        let select = select_of(
            &cx.body,
            ix.as_ref(),
            is_global(&t, ix.as_ref()),
            proj.is_some(),
        )?;
        let segment = segment_of(&cx.body)?;
        let prefix = ix
            .as_ref()
            .map_or_else(|| t.id.to_vec(), |i| index_prefix(&t, i));
        let mut plan = Plan {
            space: if ix.is_some() {
                Space::Index
            } else {
                Space::Items
            },
            sort: ix
                .as_ref()
                .map_or_else(|| t.range.clone(), |i| i.range.clone()),
            lo: prefix.clone(),
            hi: keys::prefix_end(&prefix),
            t,
            ix,
            filter,
            proj,
            select,
            limit: limit_of(&cx.body),
            forward: true,
            range: None,
            segment,
        };
        self.start_after(cx, &mut plan, None)?;
        Ok(plan)
    }

    fn choose_index(&self, cx: &Cx<'_>, t: &Table) -> Result<Option<Index>, ApiError> {
        let Some(name) = str_of(&cx.body, "IndexName") else {
            return Ok(None);
        };
        let ix = t.index(name).cloned().ok_or_else(|| {
            validation(format!(
                "The table does not have the specified index: {name}"
            ))
        })?;
        let global = t.gsis.iter().any(|g| g.name == name);
        if global && bool_of(&cx.body, "ConsistentRead").unwrap_or(false) {
            return Err(validation(
                "Consistent reads are not supported on global secondary indexes",
            ));
        }
        Ok(Some(ix))
    }

    fn filter_and_projection(
        &self,
        cx: &Cx<'_>,
        scope: &mut Scope<'_>,
    ) -> Result<(Option<Cond>, Option<Vec<Path>>), ApiError> {
        let filter = match str_of(&cx.body, "FilterExpression") {
            Some(text) => Some(parse_condition(text, Kind::Filter, scope).map_err(inv)?),
            None => None,
        };
        Ok((filter, projection(&cx.body, scope)?))
    }

    /// Moves the start of the read past `ExclusiveStartKey`. A query's start key must lie in the partition being read.
    fn start_after(
        &self,
        cx: &Cx<'_>,
        plan: &mut Plan,
        partition: Option<&[u8]>,
    ) -> Result<(), ApiError> {
        if cx.body.get("ExclusiveStartKey").is_none() {
            return Ok(());
        }
        let esk = item_member(&cx.body, "ExclusiveStartKey")?;
        let at = self.position(plan, &esk)?;
        if let Some(p) = partition
            && !at.starts_with(p)
        {
            return Err(validation(
                "The provided starting key is outside query boundaries based on provided conditions",
            ));
        }
        if plan.forward {
            let mut after = at;
            after.push(0);
            if after > plan.lo {
                plan.lo = after;
            }
        } else if at < plan.hi {
            plan.hi = at;
        }
        Ok(())
    }

    /// The bytes an `ExclusiveStartKey` stands for in the read's space.
    fn position(&self, plan: &Plan, esk: &aws_expr::Item) -> Result<Vec<u8>, ApiError> {
        let invalid = || validation("The provided starting key is invalid");
        let wanted = plan.position_keys();
        if esk.len() != wanted.len()
            || wanted.iter().any(|k| {
                esk.get(&k.name)
                    .is_none_or(|v| v.type_name() != k.ty.to_string())
            })
        {
            return Err(invalid());
        }
        let pk = pk_bytes(&plan.t, esk).map_err(|_| invalid())?;
        match &plan.ix {
            None => Ok(crate::stage::item_key(&plan.t, &pk)),
            Some(ix) => index_entry(&plan.t, ix, esk, &pk)
                .ok()
                .flatten()
                .ok_or_else(invalid),
        }
    }
}

/// `Limit`, or no limit.
fn limit_of(body: &Json) -> usize {
    int_of(body, "Limit")
        .and_then(|l| usize::try_from(l).ok())
        .unwrap_or(usize::MAX)
}

/// `Segment` and `TotalSegments`, which come together.
fn segment_of(body: &Json) -> Result<Option<(u32, u32)>, ApiError> {
    let (seg, total) = (int_of(body, "Segment"), int_of(body, "TotalSegments"));
    match (seg, total) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(bad_param(
            "The TotalSegments parameter is required but was not present in the request when Segment parameter is present",
        )),
        (None, Some(_)) => Err(bad_param(
            "The Segment parameter is required but was not present in the request when parameter TotalSegments is present",
        )),
        (Some(s), Some(n)) if s >= n => Err(bad_param(&format!(
            "The Segment parameter is zero-based and must be less than parameter TotalSegments: Segment: {s} is not less than TotalSegments: {n}"
        ))),
        (Some(s), Some(n)) => Ok(Some((
            u32::try_from(s).unwrap_or(0),
            u32::try_from(n).unwrap_or(1),
        ))),
    }
}

/// The bytes a key condition's range reads, in the order asked.
fn range_bounds(prefix: &[u8], range: Option<&Range>, forward: bool) -> (Vec<u8>, Vec<u8>) {
    let mut lo = prefix.to_vec();
    let mut hi = keys::prefix_end(prefix);
    if let Some(r) = range {
        if forward && let Some(v) = r.seek_from() {
            keys::encode(v, &mut lo);
        }
        if !forward && let Some(v) = r.seek_to() {
            let mut at = prefix.to_vec();
            keys::encode(v, &mut at);
            hi = keys::prefix_end(&at);
        }
    }
    (lo, hi)
}

/// A query's filter may not look at a key attribute. The text is `(memory)`.
fn forbid_key_filter(filter: Option<&Cond>, keys: [Option<&Key>; 4]) -> Result<(), ApiError> {
    let Some(f) = filter else { return Ok(()) };
    match f
        .roots()
        .into_iter()
        .find(|r| keys.iter().flatten().any(|k| k.name == *r))
    {
        Some(name) => Err(validation(format!(
            "Filter Expression can only contain non-primary key attributes: Primary key attribute: {name}"
        ))),
        None => Ok(()),
    }
}
