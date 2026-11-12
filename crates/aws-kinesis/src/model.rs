//! What Kinesis holds: a stream, its shards (with their lineage kept open for resharding), consumers, and a record.

use std::collections::{BTreeMap, BTreeSet};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

/// The largest hash key: the shards of a stream divide `0..=MAX_HASH` between them.
pub(crate) const MAX_HASH: u128 = u128::MAX;
/// A sequence number is the arrival time in milliseconds shifted left by this much, plus a counter that makes it strictly
/// increasing in a shard. So a time is a sequence number (`AT_TIMESTAMP`, and expiry by retention) and it sorts as a number.
pub(crate) const SEQ_SHIFT: u32 = 24;

/// One shard. `parent` and `adjacent_parent` are kept for resharding, which is refused by name for now: the lineage is
/// part of the record so that adding it later does not change what is stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Shard {
    pub(crate) id: u32,
    pub(crate) start: u128,
    pub(crate) end: u128,
    pub(crate) parent: Option<u32>,
    pub(crate) adjacent_parent: Option<u32>,
    /// The sequence number the shard starts at.
    pub(crate) first_seq: u128,
    /// The last sequence number given out, or 0.
    pub(crate) last_seq: u128,
    /// Set when the shard is closed (a split or merge): the last sequence number it will ever hold.
    pub(crate) ended_seq: Option<u128>,
}

impl Shard {
    pub(crate) fn name(&self) -> String {
        shard_name(self.id)
    }

    pub(crate) fn holds(&self, hash: u128) -> bool {
        self.ended_seq.is_none() && (self.start..=self.end).contains(&hash)
    }
}

pub(crate) fn shard_name(id: u32) -> String {
    format!("shardId-{id:012}")
}

/// `shardId-000000000003` as 3.
pub(crate) fn shard_number(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("shardId-")?;
    (digits.len() == 12 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// A registered stream consumer (enhanced fan-out). It is stored; nothing subscribes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Consumer {
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    pub(crate) tags: BTreeMap<String, String>,
}

/// A stream.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Stream {
    /// The number that prefixes the stream's rows.
    pub(crate) no: u32,
    pub(crate) name: String,
    pub(crate) stream_id: String,
    pub(crate) created_ms: i64,
    pub(crate) on_demand: bool,
    pub(crate) shards: Vec<Shard>,
    pub(crate) next_shard: u32,
    pub(crate) retention_hours: i64,
    pub(crate) kms_key: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) metrics: BTreeSet<String>,
    pub(crate) consumers: Vec<Consumer>,
    pub(crate) policy: Option<String>,
    pub(crate) max_record_kib: i64,
    pub(crate) warm_mibps: Option<i64>,
    pub(crate) distribution: Option<String>,
}

impl Stream {
    pub(crate) fn arn(&self, region: &str, account: &str) -> String {
        stream_arn(region, account, &self.name)
    }

    pub(crate) fn shard(&self, id: u32) -> Option<&Shard> {
        self.shards.iter().find(|s| s.id == id)
    }

    /// The open shards.
    pub(crate) fn open(&self) -> impl Iterator<Item = &Shard> {
        self.shards.iter().filter(|s| s.ended_seq.is_none())
    }
}

pub(crate) fn stream_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:kinesis:{region}:{account}:stream/{name}")
}

/// A stored record: what a shard holds under its sequence number.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub(crate) arrived_ms: i64,
    pub(crate) partition_key: String,
    pub(crate) data: Vec<u8>,
}

/// The row key of a record: stream, shard, sequence number, as fixed-width hex so the keys sort as the records do.
pub(crate) fn row_key(stream: u32, shard: u32, seq: u128) -> String {
    format!("{stream:08x}{shard:08x}{seq:032x}")
}

/// The hash key of a partition key: its MD5 digest read as a 128-bit number.
pub(crate) fn hash_of(partition_key: &str) -> u128 {
    let digest = Md5::digest(partition_key.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest);
    u128::from_be_bytes(bytes)
}

/// `n` equal hash ranges covering the whole key space: the last one ends at the top.
pub(crate) fn ranges(n: u32) -> Vec<(u128, u128)> {
    let n = u128::from(n.max(1));
    let width = MAX_HASH / n;
    // The space has 2^128 keys, not 2^128 - 1: the remainder of the division is spread by starting each shard at
    // `i * 2^128 / n` (computed without overflow as `i * width + i * (rem + 1) / n`).
    let rem = MAX_HASH % n + 1;
    let start = |i: u128| i * width + i * rem / n;
    (0..n)
        .map(|i| {
            let s = start(i);
            let e = if i + 1 == n {
                MAX_HASH
            } else {
                start(i + 1) - 1
            };
            (s, e)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_shards_split_the_space_where_aws_does() {
        let r = ranges(2);
        assert_eq!(r, [(0, (1u128 << 127) - 1), (1u128 << 127, MAX_HASH)]);
    }

    #[test]
    fn any_number_of_shards_tile_the_space_without_a_gap() {
        for n in [1u32, 3, 4, 7, 10, 100] {
            let r = ranges(n);
            assert_eq!(r.first().map(|x| x.0), Some(0));
            assert_eq!(r.last().map(|x| x.1), Some(MAX_HASH));
            for pair in r.windows(2) {
                if let [a, b] = pair {
                    assert_eq!(a.1 + 1, b.0, "n = {n}");
                }
            }
        }
    }

    #[test]
    fn partition_keys_land_where_the_recording_says() {
        // Recorded on AWS (test_partition_keys_route_across_shards): with two even shards.
        let upper = 1u128 << 127;
        for (key, shard) in [
            ("a", 0),
            ("b", 1),
            ("c", 0),
            ("d", 1),
            ("user-1", 1),
            ("user-2", 0),
        ] {
            assert_eq!(u8::from(hash_of(key) >= upper), shard, "{key}");
        }
    }

    #[test]
    fn shard_names_round_trip() {
        assert_eq!(shard_name(3), "shardId-000000000003");
        assert_eq!(shard_number("shardId-000000000099"), Some(99));
        assert_eq!(shard_number("shardId-9"), None);
        assert_eq!(shard_number("x"), None);
    }
}
