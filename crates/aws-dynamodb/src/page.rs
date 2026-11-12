//! Running a planned read: one page of a query or scan, with the limit, the 1 MB bound, the filter and the key to continue from.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use aws_expr::{Item, item_size, item_to_json};
use aws_wire::ApiError;
use serde_json::{Map, Value as Json};

use crate::condition::{consumed, narrowed};
use crate::core::{Core, Cx};
use crate::disk::Flow;
use crate::keys::Space;
use crate::meta::{Projection, Table};
use crate::query::{Plan, Select};
use crate::req::bool_of;
use crate::stage::{decode_item, key_of};

/// A page ends when the items it has read add up to this much.
const PAGE_BYTES: usize = 1024 * 1024;

/// What a read has gathered.
#[derive(Debug, Default)]
struct Page {
    items: Vec<Item>,
    matched: i64,
    scanned: i64,
    bytes: usize,
    last: Option<Item>,
    more: bool,
    failure: Option<ApiError>,
}

impl Core {
    pub(crate) fn query(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let plan = self.plan_query(cx, t)?;
        self.run(cx, &plan)
    }

    pub(crate) fn scan(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let t = self.table(&cx.table_name("TableName")?)?;
        let plan = self.plan_scan(cx, t)?;
        self.run(cx, &plan)
    }

    /// Reads one page.
    fn run(&self, cx: &Cx<'_>, plan: &Plan) -> Result<Json, ApiError> {
        let mut page = Page::default();
        self.disk
            .scan(plan.space, &plan.lo, &plan.hi, !plan.forward, |k, v| {
                self.visit(plan, &mut page, k, v)
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Some(e) = page.failure {
            return Err(e);
        }
        let mut out = Map::new();
        out.insert("Count".into(), page.matched.into());
        out.insert("ScannedCount".into(), page.scanned.into());
        if plan.select != Select::Count {
            let items: Vec<Json> = page
                .items
                .iter()
                .map(|i| item_to_json(&narrowed(i, plan.proj.as_deref())))
                .collect();
            out.insert("Items".into(), Json::Array(items));
        }
        if page.more
            && let Some(last) = &page.last
        {
            out.insert("LastEvaluatedKey".into(), item_to_json(last));
        }
        let consistent = bool_of(&cx.body, "ConsistentRead").unwrap_or(false);
        if let Some(c) = consumed(&cx.body, &plan.t.name, page.bytes, false, consistent) {
            out.insert("ConsumedCapacity".into(), c);
        }
        Ok(Json::Object(out))
    }

    /// Looks at one entry of the read.
    fn visit(&self, plan: &Plan, page: &mut Page, key: &[u8], value: &[u8]) -> Flow {
        let (item, pk) = match self.entry_item(plan, key, value) {
            Ok(Some(found)) => found,
            Ok(None) => return Flow::Continue,
            Err(e) => {
                page.failure = Some(e);
                return Flow::Stop;
            }
        };
        if let (Some(r), Some(sort)) = (&plan.range, &plan.sort) {
            let Some(s) = item.get(&sort.name) else {
                return Flow::Continue;
            };
            if !r.matches(s) {
                let done = if plan.forward {
                    r.done_after(s)
                } else {
                    r.done_before(s)
                };
                return if done { Flow::Stop } else { Flow::Continue };
            }
        }
        if let Some((segment, total)) = plan.segment
            && segment_of(&pk, total) != segment
        {
            return Flow::Continue;
        }
        let view = shape(plan, item);
        page.scanned += 1;
        page.bytes += item_size(&view);
        if plan.filter.as_ref().is_none_or(|f| f.eval(&view)) {
            page.matched += 1;
            page.items.push(view.clone());
        }
        page.last = Some(position_of(plan, &view));
        if page.scanned as usize >= plan.limit || page.bytes >= PAGE_BYTES {
            page.more = true;
            return Flow::Stop;
        }
        Flow::Continue
    }

    /// The item an entry stands for, and its primary key bytes. An index entry's value is the primary key; the item is fetched.
    fn entry_item(
        &self,
        plan: &Plan,
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<(Item, Vec<u8>)>, ApiError> {
        if plan.space == Space::Items {
            let pk = key.get(plan.t.id.len()..).unwrap_or_default().to_vec();
            return Ok(Some((decode_item(value)?, pk)));
        }
        let mut full = plan.t.id.to_vec();
        full.extend_from_slice(value);
        Ok(self.read_item(&full)?.map(|i| (i, value.to_vec())))
    }
}

/// The part of the item the index holds: a global index keeps its projection, a local one fetches the whole item from the table
/// unless the read asks only for what is projected.
fn shape(plan: &Plan, item: Item) -> Item {
    let Some(ix) = &plan.ix else { return item };
    let global = plan.t.gsis.iter().any(|g| g.name == ix.name);
    if !global && plan.select != Select::Projected {
        return item;
    }
    match &ix.projection {
        Projection::All => item,
        Projection::KeysOnly => keep(&plan.t, plan, &item, &[]),
        Projection::Include(names) => keep(&plan.t, plan, &item, names),
    }
}

fn keep(t: &Table, plan: &Plan, item: &Item, names: &[String]) -> Item {
    let mut out = key_of(t, item);
    for k in plan.position_keys() {
        if let Some(v) = item.get(&k.name) {
            out.insert(k.name.clone(), v.clone());
        }
    }
    for n in names {
        if let Some(v) = item.get(n) {
            out.insert(n.clone(), v.clone());
        }
    }
    out
}

/// The `LastEvaluatedKey` for an item: the table's key attributes, and the index's.
fn position_of(plan: &Plan, item: &Item) -> Item {
    plan.position_keys()
        .into_iter()
        .filter_map(|k| item.get(&k.name).map(|v| (k.name.clone(), v.clone())))
        .collect()
}

/// Which segment of a parallel scan an item falls in (FNV-1a over its key bytes).
fn segment_of(pk: &[u8], total: u32) -> u32 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in pk {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    u32::try_from(h % u64::from(total.max(1))).unwrap_or(0)
}
