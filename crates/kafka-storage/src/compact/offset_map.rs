//! For each key in the part of the log being cleaned, the offset of its latest record. A record whose key has a later
//! offset in the map is superseded. The map is bounded: when it is full, cleaning stops at the last batch it holds.

use std::collections::HashMap;

/// What an entry costs besides its key (the value, the hash table's own).
const ENTRY_OVERHEAD: usize = 56;

#[derive(Debug)]
pub(crate) struct OffsetMap {
    latest_for: HashMap<Box<[u8]>, i64>,
    used: usize,
    limit: usize,
    /// The offset of the last record the map covers: everything up to it was looked at.
    latest: i64,
}

impl OffsetMap {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            latest_for: HashMap::new(),
            used: 0,
            limit,
            latest: -1,
        }
    }

    /// Notes that `key` is at `offset` (later than any it had). `false` if the key is new and there is no room.
    pub(crate) fn put(&mut self, key: &[u8], offset: i64) -> bool {
        if let Some(slot) = self.latest_for.get_mut(key) {
            *slot = offset;
            return true;
        }
        let cost = key.len() + ENTRY_OVERHEAD;
        if self.used + cost > self.limit {
            return false;
        }
        self.used += cost;
        self.latest_for.insert(key.into(), offset);
        true
    }

    /// The offset of the latest record for `key`, if the map has one.
    pub(crate) fn get(&self, key: &[u8]) -> Option<i64> {
        self.latest_for.get(key).copied()
    }

    /// The map covers the log up to and including `offset`.
    pub(crate) fn cover_to(&mut self, offset: i64) {
        self.latest = self.latest.max(offset);
    }

    /// The last offset the map covers (-1 if none).
    pub(crate) fn latest(&self) -> i64 {
        self.latest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_put_wins_and_a_full_map_refuses_only_new_keys() {
        let mut m = OffsetMap::new(2 * (1 + ENTRY_OVERHEAD));
        assert!(m.put(b"a", 1) && m.put(b"b", 2));
        assert!(m.put(b"a", 7), "a key it has needs no room");
        assert!(!m.put(b"c", 8), "a new one does");
        assert_eq!(
            (m.get(b"a"), m.get(b"b"), m.get(b"c")),
            (Some(7), Some(2), None)
        );
        m.cover_to(5);
        m.cover_to(3);
        assert_eq!(m.latest(), 5, "never goes back");
    }
}
