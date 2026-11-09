//! Whether a record stays in the log.

use kafka_wire::batch::Seen;

use super::offset_map::OffsetMap;

/// Whether a record is left in the log (source: `Cleaner.shouldRetainRecord`): it is the latest for its key, and is not
/// a deletion whose time has come. A record past what the map covers is always left, and one with no key never is.
pub(super) fn retain(map: &OffsetMap, horizon: Option<i64>, now: i64, seen: &Seen<'_>) -> bool {
    if seen.offset > map.latest() {
        return true;
    }
    let Some(key) = seen.key else {
        return false;
    };
    let latest = map.get(key).is_none_or(|found| seen.offset >= found);
    let deletions_stay = horizon.is_none_or(|h| now < h);
    latest && (!seen.tombstone || deletions_stay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> OffsetMap {
        let mut m = OffsetMap::new(1 << 10);
        assert!(m.put(b"a", 5) && m.put(b"b", 8));
        m.cover_to(8);
        m
    }

    fn seen<'a>(offset: i64, key: Option<&'a [u8]>, tombstone: bool) -> Seen<'a> {
        Seen {
            offset,
            key,
            tombstone,
        }
    }

    #[test]
    fn a_record_stays_only_if_it_is_the_latest_of_its_key() {
        let m = map();
        assert!(retain(&m, None, 0, &seen(5, Some(b"a"), false)));
        assert!(
            !retain(&m, None, 0, &seen(2, Some(b"a"), false)),
            "a later one exists"
        );
        assert!(
            retain(&m, None, 0, &seen(1, Some(b"z"), false)),
            "a key the map has not met"
        );
    }

    #[test]
    fn a_record_with_no_key_goes() {
        assert!(!retain(&map(), None, 0, &seen(3, None, false)));
    }

    #[test]
    fn a_deletion_stays_until_its_horizon_and_goes_after() {
        let m = map();
        let t = seen(5, Some(b"a"), true);
        assert!(retain(&m, None, 1_000, &t), "no horizon yet");
        assert!(retain(&m, Some(2_000), 1_999, &t));
        assert!(!retain(&m, Some(2_000), 2_000, &t), "its time has come");
    }

    #[test]
    fn what_is_at_the_last_offset_the_map_covers_is_judged_like_the_rest() {
        // The map covers up to offset 8 whatever the record there is: a deletion past its time, or one with no key, goes.
        let m = map();
        assert!(!retain(&m, Some(0), 1_000, &seen(8, Some(b"b"), true)));
        assert!(!retain(&m, None, 0, &seen(8, None, false)));
        assert!(retain(&m, None, 0, &seen(8, Some(b"b"), false)));
    }

    #[test]
    fn what_is_past_the_map_is_left_whatever_it_is() {
        let m = map();
        assert!(
            retain(&m, Some(0), 1_000, &seen(9, Some(b"a"), true)),
            "an expired deletion past the map"
        );
        assert!(
            retain(&m, None, 0, &seen(9, None, false)),
            "no key, past the map"
        );
    }
}
