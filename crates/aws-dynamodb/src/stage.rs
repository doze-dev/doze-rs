//! The one way an item changes: `stage` turns "this item was X and is now Y" into the writes it needs (the item, its index entries,
//! its time-to-live entry, its stream record), after the checks every write shares.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use aws_expr::{Item, MAX_DEPTH, Number, Value, item_size};
use aws_wire::ApiError;
use doze_store::record;

use crate::core::Core;
use crate::keys::{self, Space};
use crate::meta::{Index, Key, Table};
use crate::req::{bad_param, validation};

/// The most one item may hold, by AWS's count.
pub(crate) const MAX_ITEM_BYTES: usize = 400 * 1024;
const MAX_HASH_BYTES: usize = 2048;
const MAX_RANGE_BYTES: usize = 1024;
const ITEM_VERSION: u8 = 1;

pub(crate) fn encode_item(item: &Item) -> Result<Vec<u8>, ApiError> {
    record::encode("item", ITEM_VERSION, item).map_err(|e| ApiError::internal(e.to_string()))
}

pub(crate) fn decode_item(bytes: &[u8]) -> Result<Item, ApiError> {
    record::decode("item", ITEM_VERSION, bytes).map_err(|e| ApiError::internal(e.to_string()))
}

fn empty_key(name: &str, what: &str) -> ApiError {
    validation(format!(
        "One or more parameter values are not valid. The AttributeValue for a key attribute cannot contain an empty {what} value. Key: {name}"
    ))
}

/// Checks one key attribute of an item and writes its bytes. `size_limit` is 2,048 for a partition key and 1,024 for a sort key.
fn key_part(key: &Key, item: &Item, out: &mut Vec<u8>, limit: usize) -> Result<(), ApiError> {
    let value = item
        .get(&key.name)
        .ok_or_else(|| bad_param(&format!("Missing the key {} in the item", key.name)))?;
    let actual = value.type_name();
    if actual != key.ty.to_string() {
        return Err(bad_param(&format!(
            "Type mismatch for key {} expected: {} actual: {actual}",
            key.name, key.ty
        )));
    }
    let size = match value {
        Value::S(s) if s.is_empty() => return Err(empty_key(&key.name, "string")),
        Value::B(b) if b.is_empty() => return Err(empty_key(&key.name, "binary")),
        Value::S(s) => s.len(),
        Value::B(b) => b.len(),
        _ => 0,
    };
    if size > limit {
        return Err(bad_param(&if limit == MAX_HASH_BYTES {
            format!("Size of hashkey has exceeded the maximum size limit of{MAX_HASH_BYTES} bytes")
        } else {
            format!(
                "Aggregated size of all range keys has exceeded the size limit of {MAX_RANGE_BYTES} bytes"
            )
        }));
    }
    keys::encode(value, out);
    Ok(())
}

/// The bytes of an item's primary key, after checking the key attributes it must hold.
pub(crate) fn pk_bytes(t: &Table, item: &Item) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    key_part(&t.hash, item, &mut out, MAX_HASH_BYTES)?;
    if let Some(r) = &t.range {
        key_part(r, item, &mut out, MAX_RANGE_BYTES)?;
    }
    Ok(out)
}

/// The primary key bytes of a `Key` parameter: exactly the table's key attributes, of the right types.
pub(crate) fn key_bytes(t: &Table, key: &Item) -> Result<Vec<u8>, ApiError> {
    let wanted = 1 + usize::from(t.range.is_some());
    let schema = || validation("The provided key element does not match the schema");
    if key.len() != wanted {
        return Err(schema());
    }
    for k in std::iter::once(&t.hash).chain(t.range.as_ref()) {
        match key.get(&k.name) {
            Some(v) if v.type_name() == k.ty.to_string() => {}
            _ => return Err(schema()),
        }
    }
    pk_bytes(t, key)
}

/// The item's key attributes only.
pub(crate) fn key_of(t: &Table, item: &Item) -> Item {
    std::iter::once(&t.hash)
        .chain(t.range.as_ref())
        .filter_map(|k| item.get(&k.name).map(|v| (k.name.clone(), v.clone())))
        .collect()
}

/// The full key of an item in the items space: the table's id, then its primary key.
pub(crate) fn item_key(t: &Table, pk: &[u8]) -> Vec<u8> {
    let mut key = t.id.to_vec();
    key.extend_from_slice(pk);
    key
}

/// The prefix every entry of an index is filed under.
pub(crate) fn index_prefix(t: &Table, ix: &Index) -> Vec<u8> {
    let mut p = t.id.to_vec();
    p.extend_from_slice(&ix.id.to_be_bytes());
    p
}

/// The key under which an item appears in an index: its index key values, then its primary key. `None` when the item lacks an index
/// key attribute (an index is sparse); an error when it has one of the wrong type or an empty one.
pub(crate) fn index_entry(
    t: &Table,
    ix: &Index,
    item: &Item,
    pk: &[u8],
) -> Result<Option<Vec<u8>>, ApiError> {
    let mut out = index_prefix(t, ix);
    for k in std::iter::once(&ix.hash).chain(ix.range.as_ref()) {
        let Some(v) = item.get(&k.name) else {
            return Ok(None);
        };
        if v.type_name() != k.ty.to_string() {
            return Err(bad_param(&format!(
                "Type mismatch for Index Key {} Expected: {} Actual: {} IndexName: {}",
                k.name,
                k.ty,
                v.type_name(),
                ix.name
            )));
        }
        if matches!(v, Value::S(s) if s.is_empty()) || matches!(v, Value::B(b) if b.is_empty()) {
            return Err(validation(format!(
                "One or more parameter values are not valid. A value specified for a secondary index key is not supported. The AttributeValue for a key attribute cannot contain an empty string value. IndexName: {}, IndexKey: {}",
                ix.name, k.name
            )));
        }
        keys::encode(v, &mut out);
    }
    out.extend_from_slice(pk);
    Ok(Some(out))
}

/// The key under which an item waits for its time to live to pass: when it expires, then its primary key.
fn ttl_entry(t: &Table, item: &Item, pk: &[u8]) -> Option<Vec<u8>> {
    if !t.ttl.enabled {
        return None;
    }
    let Some(Value::N(n)) = item.get(&t.ttl.attribute) else {
        return None;
    };
    let secs = n.floor_i64()?;
    let mut out = t.id.to_vec();
    out.extend_from_slice(&0_u32.to_be_bytes());
    Number::from_i64(secs).order_key(&mut out);
    out.extend_from_slice(pk);
    Some(out)
}

/// Every check that an item may be written to the table.
pub(crate) fn validate_item(t: &Table, item: &Item) -> Result<Vec<u8>, ApiError> {
    let pk = pk_bytes(t, item)?;
    for ix in t.gsis.iter().chain(&t.lsis) {
        index_entry(t, ix, item, &pk)?;
    }
    if item.values().any(|v| v.depth() > MAX_DEPTH) {
        return Err(bad_param("Nesting Levels have exceeded supported limits"));
    }
    if item_size(item) > MAX_ITEM_BYTES {
        return Err(validation(
            "Item size has exceeded the maximum allowed size",
        ));
    }
    Ok(pk)
}

impl Core {
    /// The item stored under `key` in the items space.
    pub(crate) fn read_item(&self, key: &[u8]) -> Result<Option<Item>, ApiError> {
        self.disk
            .get(Space::Items, key)
            .map_err(|e| ApiError::internal(e.to_string()))?
            .map(|b| decode_item(&b))
            .transpose()
    }

    /// Records that an item went from `old` to `new` (either may be absent): the writes for the item, its index entries, its
    /// time-to-live entry and its stream record. `new` must already have passed [`validate_item`].
    pub(crate) fn stage(
        &mut self,
        t: &Table,
        now_ms: i64,
        old: Option<&Item>,
        new: Option<&Item>,
        by_ttl: bool,
    ) -> Result<(), ApiError> {
        let Some(basis) = new.or(old) else {
            return Ok(());
        };
        let pk = pk_bytes(t, basis)?;
        let key = item_key(t, &pk);
        match new {
            Some(n) => self.pending.put(Space::Items, key, encode_item(n)?),
            None => self.pending.del(Space::Items, key),
        }
        for ix in t.gsis.iter().chain(&t.lsis) {
            let before = old
                .map(|o| index_entry(t, ix, o, &pk))
                .transpose()?
                .flatten();
            let after = new
                .map(|n| index_entry(t, ix, n, &pk))
                .transpose()?
                .flatten();
            self.move_entry(Space::Index, before, after, &pk);
        }
        let before = old.and_then(|o| ttl_entry(t, o, &pk));
        let after = new.and_then(|n| ttl_entry(t, n, &pk));
        self.move_entry(Space::Index, before, after, &pk);
        if old != new {
            self.append_stream(t, now_ms, old, new, by_ttl)?;
        }
        Ok(())
    }

    /// Moves an index entry: the old key goes, the new one is written (the value is the item's primary key).
    fn move_entry(
        &mut self,
        space: Space,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
        pk: &[u8],
    ) {
        if before == after {
            return;
        }
        if let Some(b) = before {
            self.pending.del(space, b);
        }
        if let Some(a) = after {
            self.pending.put(space, a, pk.to_vec());
        }
    }
}
