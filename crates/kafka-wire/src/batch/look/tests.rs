use std::io::Write;

use super::*;
use crate::batch::build::{BatchBuilder, Rec, encode_records};

fn rec(i: i64, key: Option<&str>, value: Option<&[u8]>, headers: &[(&str, &str)]) -> Rec {
    Rec {
        timestamp_delta: i * 10,
        offset_delta: None,
        key: key.map(|k| k.as_bytes().to_vec()),
        value: value.map(<[u8]>::to_vec),
        headers: headers
            .iter()
            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
            .collect(),
    }
}

fn recs() -> Vec<Rec> {
    vec![
        rec(0, Some("k0"), Some(b"hello"), &[("h", "1"), ("trace", "abc")]),
        rec(1, None, Some(&[0xff, 0x00, 0xfe]), &[]),
        rec(2, Some("k2"), None, &[]),
    ]
}

fn batch(attributes: u16, body: Vec<u8>) -> Batch {
    Batch::parse(
        BatchBuilder::new()
            .base_offset(500)
            .records(3)
            .attributes(attributes)
            .timestamps(1_000, 1_020)
            .body(body)
            .build(),
    )
    .unwrap()
}

type Seen = (i64, i64, Option<Vec<u8>>, Option<Vec<u8>>, usize);

fn all(b: &Batch, cap: u64) -> Result<Vec<Seen>, RecordsRefused> {
    let mut out = Vec::new();
    b.each_record_full(cap, |r| {
        out.push((
            r.offset,
            r.timestamp,
            r.key.map(<[u8]>::to_vec),
            r.value.map(<[u8]>::to_vec),
            r.headers.len(),
        ));
        true
    })?;
    Ok(out)
}

#[test]
fn every_field_of_every_record_is_read_with_its_offset_and_time() {
    let seen = all(&batch(0, encode_records(&recs())), 1 << 20).unwrap();
    assert_eq!(
        seen,
        vec![
            (500, 1_000, Some(b"k0".to_vec()), Some(b"hello".to_vec()), 2),
            (501, 1_010, None, Some(vec![0xff, 0x00, 0xfe]), 0),
            (502, 1_020, Some(b"k2".to_vec()), None, 0),
        ]
    );
}

#[test]
fn headers_come_back_in_order_with_their_bytes() {
    let b = batch(0, encode_records(&recs()));
    let mut got = Vec::new();
    b.each_record_full(1 << 20, |r| {
        got.extend(r.headers.iter().map(|(k, v)| (k.to_vec(), v.map(<[u8]>::to_vec))));
        true
    })
    .unwrap();
    assert_eq!(
        got,
        vec![
            (b"h".to_vec(), Some(b"1".to_vec())),
            (b"trace".to_vec(), Some(b"abc".to_vec())),
        ]
    );
}

#[test]
fn the_walk_stops_when_the_callback_says_so() {
    let b = batch(0, encode_records(&recs()));
    let mut n = 0;
    b.each_record_full(1 << 20, |_| {
        n += 1;
        n < 2
    })
    .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn a_gzip_batch_is_read_through_its_codec_and_held_to_the_limit() {
    let plain = encode_records(&recs());
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&plain).unwrap();
    let zipped = e.finish().unwrap();
    let b = batch(1, zipped);
    assert_eq!(all(&b, 1 << 20).unwrap().len(), 3);
    let refused = all(&b, 10).unwrap_err();
    assert_eq!(refused.refusal, Refusal::MessageTooLarge);
}

#[test]
fn a_record_that_runs_past_its_length_or_is_cut_short_is_an_error_not_a_guess() {
    let mut body = encode_records(&recs());
    // Cut the section short in the middle of the last record.
    body.truncate(body.len() - 3);
    assert!(all(&batch(0, body), 1 << 20).is_err());
    // A record whose length says more than the section holds.
    let mut lying = encode_records(&recs());
    lying[0] = 0x7e;
    assert!(all(&batch(0, lying), 1 << 20).is_err());
    // A header count the record cannot hold.
    let bad = vec![
        // length 8: attributes, ts, offset, key -1, value -1, header count 0x7f..., padding
        0x10, 0x00, 0x00, 0x00, 0x01, 0x01, 0xfe, 0xff, 0x03,
    ];
    assert!(all(&batch(0, bad), 1 << 20).is_err());
}

#[test]
fn no_byte_string_makes_the_walk_panic() {
    for seed in 0..3000_u32 {
        let body: Vec<u8> = (0..40_u32)
            .map(|i| {
                (seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(i.wrapping_mul(40_503))
                    >> 13) as u8
            })
            .collect();
        let _ = all(&batch(0, body), 1 << 16);
    }
}
