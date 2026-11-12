use super::*;
use crate::batch::build::{BatchBuilder, Rec};
use crate::batch::{Batch, ProduceRules};

const RULES: ProduceRules = ProduceRules {
    version: 9,
    max_batch_bytes: 1 << 20,
};

fn rec(r: &OneRecord<'_>) -> Rec {
    Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: r.key.map(<[u8]>::to_vec),
        value: r.value.map(<[u8]>::to_vec),
        headers: r
            .headers
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.to_vec()))
            .collect(),
    }
}

/// The batch the test-support builder (written from the format) makes for the same record, but for the leader epoch,
/// which a client leaves at -1.
fn oracle(r: &OneRecord<'_>) -> Vec<u8> {
    let mut v = BatchBuilder::from_records(r.timestamp_ms, &[rec(r)])
        .build()
        .to_vec();
    v[12..16].copy_from_slice(&(-1_i32).to_be_bytes());
    crate::batch::build::reseal(&mut v);
    v
}

#[test]
fn a_batch_is_what_the_format_says_byte_for_byte() {
    let headers: [(&str, &[u8]); 2] = [("content-type", b"application/json"), ("empty", b"")];
    let big = vec![b'x'; 70_000];
    for r in [
        OneRecord {
            timestamp_ms: 1_700_000_000_123,
            key: Some(b"ord-88420"),
            value: Some(b"{\"a\":1}"),
            headers: &headers,
        },
        OneRecord {
            timestamp_ms: 0,
            key: None,
            value: Some(b"v"),
            headers: &[],
        },
        OneRecord {
            timestamp_ms: 5,
            key: Some(b"k"),
            value: None,
            headers: &[],
        },
        OneRecord {
            timestamp_ms: -1,
            key: Some(b""),
            value: Some(&big),
            headers: &headers,
        },
        OneRecord {
            timestamp_ms: i64::MAX,
            key: None,
            value: None,
            headers: &[],
        },
    ] {
        assert_eq!(single_record(&r).to_vec(), oracle(&r), "{r:?}");
    }
}

#[test]
fn the_broker_s_own_parser_takes_it_as_a_producers_batch() {
    let r = OneRecord {
        timestamp_ms: 42,
        key: Some(b"k"),
        value: Some(b"v"),
        headers: &[("h", b"1")],
    };
    let batch = Batch::from_produce(&single_record(&r), RULES).unwrap();
    assert_eq!(batch.record_count(), 1);
    assert_eq!(batch.first_timestamp().get(), 42);
    assert!(batch.producer().is_none());
}

#[test]
fn a_flipped_byte_breaks_the_checksum() {
    let r = OneRecord {
        timestamp_ms: 1,
        key: None,
        value: Some(b"v"),
        headers: &[],
    };
    let mut v = single_record(&r).to_vec();
    let last = v.len() - 1;
    v[last] ^= 1;
    assert!(Batch::from_produce(&Bytes::from(v), RULES).is_err());
}
