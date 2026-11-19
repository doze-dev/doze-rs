//! The conversions held to bytes Kafka's own code wrote (`tests/data/legacy-vectors.txt`, made by
//! `tools/old-profiles-oracle/Vectors.java` with kafka-clients 3.9.1).

use bytes::Bytes;

use super::*;
use crate::batch::{Compression, Refusal};

fn vectors() -> Vec<(String, Bytes, Option<Bytes>)> {
    include_str!("../../../tests/data/legacy-vectors.txt")
        .lines()
        .map(|l| {
            let (name, rest) = l.rsplit_once(' ').unwrap();
            if let Some((head, input)) = l.split_once(" in ") {
                let (name, out) = head.rsplit_once(' ').unwrap();
                return (name.to_owned(), hex(out), Some(hex(input)));
            }
            (name.to_owned(), hex(rest), None)
        })
        .collect()
}

fn vector(name: &str) -> (Bytes, Option<Bytes>) {
    let (_, a, b) = vectors()
        .into_iter()
        .find(|(n, ..)| n == name)
        .unwrap_or_else(|| panic!("{name}"));
    (a, b)
}

fn hex(s: &str) -> Bytes {
    Bytes::from(
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>(),
    )
}

const STRICT: UpRules = UpRules {
    strict: true,
    knows_zstd: true,
    max_message_bytes: 1 << 20,
    max_inflated: 1 << 20,
};

fn converted(set: &Bytes, rules: &UpRules) -> (Vec<Rec>, Compression) {
    match up(set, rules).unwrap() {
        Up::Converted { batch, source } => (records::read(&batch, 1 << 20).unwrap(), source),
        other => panic!("{other:?}"),
    }
}

fn kv(r: &Rec) -> (Option<&[u8]>, Option<&[u8]>) {
    (r.key.as_deref(), r.value.as_deref())
}

#[test]
fn every_message_set_kafka_writes_is_read_with_its_records_times_and_codec() {
    for magic in ["m0", "m1"] {
        for (codec, want) in [
            ("none", Compression::None),
            ("gzip", Compression::Gzip),
            ("snappy", Compression::Snappy),
            ("lz4", Compression::Lz4),
        ] {
            let (set, _) = vector(&format!("legacy {magic} {codec}"));
            let (recs, source) = converted(&set, &STRICT);
            assert_eq!(source, want, "{magic} {codec}");
            assert_eq!(recs.len(), 3, "{magic} {codec}");
            assert_eq!(kv(&recs[0]), (Some(&b"k0"[..]), Some(&b"v0"[..])));
            assert_eq!(kv(&recs[1]), (None, Some(&b"v1"[..])));
            assert_eq!(kv(&recs[2]), (Some(&b"k2"[..]), None));
            let times: Vec<i64> = recs.iter().map(|r| r.timestamp).collect();
            let want_times = if magic == "m0" {
                vec![-1, -1, -1]
            } else {
                vec![1_600_000_000_000, 1_600_000_000_001, 1_600_000_000_002]
            };
            assert_eq!(times, want_times, "{magic} {codec}");
            assert!(recs.iter().all(|r| r.headers.is_empty()));
        }
    }
}

#[test]
fn a_wrapper_under_log_append_time_gives_its_time_to_every_inner_message() {
    // Kafka's builder writes a LogAppendTime set uncompressed as messages each with the time and the type bit.
    let (set, _) = vector("legacy m1 none append");
    let (recs, _) = converted(&set, &STRICT);
    assert!(recs.iter().all(|r| r.timestamp == 1_600_000_000_500));
}

#[test]
fn the_v2_batch_made_is_kafkas_shape() {
    let (set, _) = vector("legacy m0 none");
    let Up::Converted { batch, .. } = up(&set, &STRICT).unwrap() else {
        panic!()
    };
    assert_eq!(batch.base_offset().map(|o| o.get()), Some(0));
    assert_eq!(batch.record_count(), 3);
    assert_eq!(batch.producer(), None);
    assert_eq!(batch.first_timestamp().get(), -1);
    assert_eq!(batch.max_timestamp().get(), -1);
    assert_eq!(batch.attributes().compression(), Compression::None);
}

#[test]
fn nothing_whole_is_nothing_to_append_and_a_cut_last_message_is_not_read() {
    assert_eq!(up(&Bytes::new(), &STRICT).unwrap(), Up::Empty);
    let (set, _) = vector("legacy m1 none");
    assert_eq!(up(&set.slice(..10), &STRICT).unwrap(), Up::Empty);
    let (recs, _) = converted(&set.slice(..set.len() - 3), &STRICT);
    assert_eq!(recs.len(), 2);
}

#[test]
fn a_one_batch_set_of_format_v2_is_taken_as_it_is() {
    let (_, Some(v2)) = vector("down CreateTime m0 from 10") else {
        panic!()
    };
    let mut at_zero = v2.to_vec();
    at_zero[..8].copy_from_slice(&0_i64.to_be_bytes());
    let at_zero = Bytes::from(at_zero);
    assert_eq!(up(&at_zero, &STRICT).unwrap(), Up::Batch(at_zero.clone()));
    // A client's batch has base offset 0.
    assert_eq!(
        up(&v2, &STRICT).unwrap_err().refusal,
        Refusal::InvalidRecord
    );
}

fn concat(parts: &[&Bytes]) -> Bytes {
    Bytes::from(
        parts
            .iter()
            .flat_map(|p| p.iter().copied())
            .collect::<Vec<u8>>(),
    )
}

#[test]
fn from_kafka_2_a_set_is_one_format_and_a_compressed_one_one_wrapper_and_kafka_1_takes_any_mix() {
    let (m0, _) = vector("legacy m0 none");
    let (m1, _) = vector("legacy m1 none");
    let (gz, _) = vector("legacy m1 gzip");
    let one_one = UpRules {
        strict: false,
        knows_zstd: false,
        ..STRICT
    };
    for mix in [concat(&[&m0, &m1]), concat(&[&gz, &m1])] {
        assert_eq!(
            up(&mix, &STRICT).unwrap_err().refusal,
            Refusal::InvalidRecord
        );
        let (recs, _) = converted(&mix, &one_one);
        assert_eq!(recs.len(), 6);
    }
    // Kafka 1.1 writes the mix in the codec of its compressed entries.
    assert_eq!(
        converted(&concat(&[&gz, &m1]), &one_one).1,
        Compression::Gzip
    );
}

#[test]
fn checksums_sizes_and_codecs_are_refused_as_kafka_refuses_them() {
    let (m1, _) = vector("legacy m1 none");
    let mut bad = m1.to_vec();
    bad[30] ^= 0x5a;
    assert_eq!(
        up(&Bytes::from(bad), &STRICT).unwrap_err().refusal,
        Refusal::CorruptMessage
    );
    let small = UpRules {
        max_message_bytes: 20,
        ..STRICT
    };
    assert_eq!(
        up(&m1, &small).unwrap_err().refusal,
        Refusal::MessageTooLarge
    );
    // A wrapper that names zstd: unknown to Kafka 1.1, and not a legacy codec to 2.0 on.
    let (gz, _) = vector("legacy m1 gzip");
    let mut z = gz.to_vec();
    z[17] = (z[17] & !7) | 4;
    let crc = checksum::crc32(&z[16..]);
    z[12..16].copy_from_slice(&crc.to_be_bytes());
    let z = Bytes::from(z);
    assert_eq!(up(&z, &STRICT).unwrap_err().refusal, Refusal::InvalidRecord);
    let old = UpRules {
        knows_zstd: false,
        strict: false,
        ..STRICT
    };
    assert_eq!(
        up(&z, &old).unwrap_err().refusal,
        Refusal::UnknownServerError
    );
    let mut size = m1.to_vec();
    size[8..12].copy_from_slice(&13_i32.to_be_bytes());
    assert_eq!(
        up(&Bytes::from(size), &STRICT).unwrap_err().refusal,
        Refusal::CorruptMessage
    );
}

fn down_rules(to: Magic, fetch_offset: i64, how: Downconversion) -> DownRules {
    DownRules {
        to,
        fetch_offset,
        how,
        max_inflated: 1 << 20,
    }
}

#[test]
fn an_uncompressed_batch_is_down_converted_byte_for_byte_as_kafka_does() {
    for t in ["CreateTime", "LogAppendTime"] {
        for (m, magic) in [("m0", Magic::V0), ("m1", Magic::V1)] {
            for from in [10, 11] {
                let name = format!("down {t} {m} from {from}");
                let (want, Some(input)) = vector(&name) else {
                    panic!()
                };
                let got = down(&input, &down_rules(magic, from, Downconversion::Eager)).unwrap();
                assert_eq!(got, want, "{name}");
            }
        }
    }
}

#[test]
fn a_compressed_batch_is_one_wrapper_in_its_codec_numbered_as_its_format_numbers_them() {
    // A compressed message set of format v1 read up, written down again as v0 and v1, read up again.
    for codec in ["gzip", "snappy", "lz4"] {
        let (set, _) = vector(&format!("legacy m1 {codec}"));
        let Up::Converted { batch, source } = up(&set, &STRICT).unwrap() else {
            panic!()
        };
        let packed = batch
            .recode(source, 1 << 20, batch.max_timestamp())
            .unwrap();
        // As the log holds it, from offset 10.
        let mut at_ten = packed.as_bytes().to_vec();
        at_ten[..8].copy_from_slice(&10_i64.to_be_bytes());
        let at_ten = Bytes::from(at_ten);
        for magic in [Magic::V0, Magic::V1] {
            let out = down(&at_ten, &down_rules(magic, 11, Downconversion::Eager)).unwrap();
            let entries: Vec<_> = message::entries(&out).collect::<Result<_, _>>().unwrap();
            assert_eq!(entries.len(), 1, "{codec} {magic:?}: one wrapper");
            let message::Entry::Message(w) = &entries[0] else {
                panic!()
            };
            assert_eq!(w.offset(), 12, "the wrapper's offset is the last record's");
            assert_eq!(codec::of(w.codec()), source);
            // The wrapper holds every record, the one before the fetch offset too.
            let (recs, _) = converted(&out, &STRICT);
            assert_eq!(recs.len(), 3, "{codec} {magic:?}");
            let (_, value) = w.key_value().unwrap();
            let inner =
                Bytes::from(codec::inflate(magic, source, &value.unwrap(), 1 << 20).unwrap());
            let offsets: Vec<i64> = message::entries(&inner)
                .map(|e| match e.unwrap() {
                    message::Entry::Message(m) => m.offset(),
                    message::Entry::Batch(_) => panic!(),
                })
                .collect();
            let want = if magic == Magic::V0 {
                [10, 11, 12]
            } else {
                [0, 1, 2]
            };
            assert_eq!(offsets, want);
        }
    }
}

#[test]
fn an_lz4_frame_of_format_v0_has_kafkas_header_checksum_both_ways() {
    let (set, _) = vector("legacy m0 lz4");
    // Kafka's frame: magic, flags 0x60, descriptor 0x40, then its checksum over all six bytes.
    let frame_at = 12 + 14;
    assert_eq!(
        &set[frame_at..frame_at + 6],
        &[0x04, 0x22, 0x4d, 0x18, 0x60, 0x40]
    );
    let kafkas = set[frame_at + 6];
    assert_eq!(
        kafkas,
        (checksum::xxh32(&set[frame_at..frame_at + 6]) >> 8) as u8
    );
    let (recs, _) = converted(&set, &STRICT);
    assert_eq!(recs.len(), 3);
    let written = codec::deflate(Magic::V0, Compression::Lz4, b"some bytes").unwrap();
    assert_eq!(written[6], (checksum::xxh32(&written[..6]) >> 8) as u8);
    assert_eq!(
        codec::inflate(Magic::V0, Compression::Lz4, &written, 100).unwrap(),
        b"some bytes"
    );
}

fn control_batch() -> Bytes {
    crate::batch::build::BatchBuilder::new()
        .attributes(0x30)
        .producer(7, 0, -1)
        .base_offset(2)
        .build()
}

#[test]
fn from_kafka_2_the_answer_is_as_long_as_the_read_and_filled_with_overflow() {
    let (want_v0, Some(input)) = vector("down CreateTime m0 from 10") else {
        panic!()
    };
    // The read is 94 bytes; the three messages 90.
    let got = down(&input, &down_rules(Magic::V0, 10, Downconversion::Chunked)).unwrap();
    assert_eq!(got.len(), input.len());
    assert_eq!(&got[..want_v0.len()], &want_v0[..]);
    // A few bytes short: the start of an overflow message (its offset, -1), cut where the answer ends.
    let short = &got[want_v0.len()..];
    assert!(!short.is_empty() && short.len() <= 8 && short.iter().all(|b| *b == 0xff));
    // From further on, more short: a whole overflow message head, a length one past what is left (at least a batch header,
    // 61), then zeros.
    let later = down(&input, &down_rules(Magic::V0, 11, Downconversion::Chunked)).unwrap();
    let (want, _) = vector("down CreateTime m0 from 11");
    let tail = &later[want.len()..];
    assert_eq!(tail.len(), input.len() - want.len());
    assert_eq!(&tail[..8], &(-1_i64).to_be_bytes());
    assert_eq!(
        i32::from_be_bytes(tail[8..12].try_into().unwrap()) as usize,
        (tail.len() + 1).max(61)
    );
    assert!(tail[12..].iter().all(|b| *b == 0));
    // v1 messages are longer than the batch was: the answer is the first batch's, whole.
    let (want_v1, _) = vector("down CreateTime m1 from 10");
    let got = down(&input, &down_rules(Magic::V1, 10, Downconversion::Chunked)).unwrap();
    assert_eq!(got, want_v1);
}

#[test]
fn the_first_batch_that_converts_after_one_that_does_not_is_converted_twice_as_kafka_does() {
    let (one, Some(input)) = vector("down CreateTime m0 from 10") else {
        panic!()
    };
    let read = concat(&[&control_batch(), &input]);
    let got = down(&read, &down_rules(Magic::V0, 10, Downconversion::Chunked)).unwrap();
    assert_eq!(got.len(), read.len());
    assert_eq!(&got[..one.len()], &one[..]);
    // Then the batches from the second read on: the same one again, cut at the read's length.
    assert!(got.len() < 2 * one.len());
    assert_eq!(&got[one.len()..], &one[..got.len() - one.len()]);
    // Kafka 1.1 converts once.
    let eager = down(&read, &down_rules(Magic::V0, 10, Downconversion::Eager)).unwrap();
    assert_eq!(eager, one);
}

#[test]
fn a_read_that_converts_to_nothing_or_holds_no_whole_batch_is_answered_as_each_release_does() {
    let marker = control_batch();
    assert_eq!(
        down(&marker, &down_rules(Magic::V0, 2, Downconversion::Eager)).unwrap(),
        marker
    );
    let overflow = down(&marker, &down_rules(Magic::V0, 2, Downconversion::Chunked)).unwrap();
    assert_eq!(overflow.len(), 12);
    assert_eq!(&overflow[..8], &(-1_i64).to_be_bytes());
    assert_eq!(&overflow[8..12], &61_i32.to_be_bytes());
    let cut = marker.slice(..40);
    assert_eq!(
        down(&cut, &down_rules(Magic::V0, 2, Downconversion::Eager)).unwrap(),
        cut
    );
    assert_eq!(
        down(&cut, &down_rules(Magic::V0, 2, Downconversion::Chunked)).unwrap(),
        cut
    );
    assert!(
        down(
            &cut,
            &down_rules(Magic::V0, 2, Downconversion::ChunkedAlways)
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn zstd_cannot_be_down_converted() {
    let (_, Some(input)) = vector("down CreateTime m0 from 10") else {
        panic!()
    };
    let batch = crate::batch::Batch::parse(input).unwrap();
    let z = batch
        .recode(Compression::Zstd, 1 << 20, batch.max_timestamp())
        .unwrap();
    assert_eq!(
        down(
            z.as_bytes(),
            &down_rules(Magic::V1, 10, Downconversion::Chunked)
        ),
        Err(DownError::Zstd)
    );
}
