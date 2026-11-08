use std::io::Write;

use crate::batch::Batch;
use crate::batch::Refusal;
use crate::batch::build::{BatchBuilder, Rec, encode_records};
use crate::batch::scan::{RecordsRefused, ScanRules};
use crate::ids::TimestampMs;

const NOW: i64 = 1_000_000;

fn rules(max_inflated: u64) -> ScanRules {
    ScanRules {
        require_keys: false,
        log_append_time: false,
        now: TimestampMs::from_wire(NOW),
        before_max_ms: i64::MAX,
        after_max_ms: i64::MAX,
        max_inflated,
        renumbering: false,
    }
}

fn recs(n: usize) -> Vec<Rec> {
    (0..n)
        .map(|i| Rec {
            timestamp_delta: i as i64,
            offset_delta: None,
            key: Some(vec![b'k'; 8]),
            value: Some(vec![b'v'; 100]),
            headers: vec![],
        })
        .collect()
}

/// A batch whose records section is `body` compressed by `codec` (attribute bits 0-2).
fn batch(codec: u16, body: Vec<u8>, count: usize) -> Batch {
    Batch::parse(
        BatchBuilder::new()
            .records(count as u32)
            .attributes(codec)
            .timestamps(NOW, NOW)
            .body(body)
            .build(),
    )
    .unwrap()
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn xerial(data: &[u8], block: usize) -> Vec<u8> {
    let mut out = vec![
        0x82, b'S', b'N', b'A', b'P', b'P', b'Y', 0, 0, 0, 0, 1, 0, 0, 0, 1,
    ];
    for chunk in data.chunks(block) {
        let compressed = snap::raw::Encoder::new().compress_vec(chunk).unwrap();
        out.extend((compressed.len() as u32).to_be_bytes());
        out.extend(compressed);
    }
    out
}

fn lz4(data: &[u8]) -> Vec<u8> {
    let mut e = lz4_flex::frame::FrameEncoder::new(Vec::new());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn zstd(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

#[test]
fn every_codec_a_client_uses_is_read_in_the_framing_that_client_writes() {
    let plain = encode_records(&recs(40));
    let raw_snappy = snap::raw::Encoder::new().compress_vec(&plain).unwrap();
    let cases: Vec<(&str, u16, Vec<u8>)> = vec![
        ("gzip", 1, gzip(&plain)),
        (
            "snappy as the Java client writes it, in several blocks",
            2,
            xerial(&plain, 700),
        ),
        (
            "snappy as librdkafka writes it, one plain block",
            2,
            raw_snappy,
        ),
        ("lz4", 3, lz4(&plain)),
        ("zstd", 4, zstd(&plain)),
    ];
    for (label, codec, body) in cases {
        let scanned = batch(codec, body, 40).scan(&rules(1 << 20), &"t-0");
        assert_eq!(scanned.unwrap().max_timestamp.get(), NOW + 39, "{label}");
    }
}

#[test]
fn a_gzip_stream_of_several_members_is_all_read() {
    let plain = encode_records(&recs(10));
    let (a, b) = plain.split_at(plain.len() / 2);
    let two_members = [gzip(a), gzip(b)].concat();
    assert!(
        batch(1, two_members, 10)
            .scan(&rules(1 << 20), &"t-0")
            .is_ok()
    );
}

#[test]
fn what_inflates_past_the_limit_is_too_large_however_little_it_weighs() {
    // One record whose value is eight megabytes of zeros: a few hundred bytes compressed.
    let one = Rec {
        timestamp_delta: 0,
        offset_delta: None,
        key: None,
        value: Some(vec![0u8; 8 << 20]),
        headers: vec![],
    };
    let bomb = encode_records(&[one]);
    for (label, codec, body) in [
        ("gzip", 1, gzip(&bomb)),
        ("snappy", 2, xerial(&bomb, 64 << 10)),
        ("lz4", 3, lz4(&bomb)),
        ("zstd", 4, zstd(&bomb)),
    ] {
        assert!(
            body.len() < (1 << 20),
            "{label}: the bomb weighs {} bytes",
            body.len()
        );
        let refused: RecordsRefused = batch(codec, body, 1)
            .scan(&rules(1 << 20), &"t-0")
            .unwrap_err();
        assert_eq!(
            (refused.refusal, refused.before_log),
            (Refusal::MessageTooLarge, true),
            "{label}: {}",
            refused.reason
        );
    }
}

#[test]
fn bytes_that_are_not_their_codecs_are_unreadable_and_a_truncated_stream_is_too() {
    for (label, codec, body) in [
        ("gzip", 1, b"not gzip at all, no".to_vec()),
        (
            "snappy",
            2,
            vec![
                0x82, b'S', b'N', b'A', b'P', b'P', b'Y', 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 9, 1,
                2,
            ],
        ),
        ("lz4", 3, b"not lz4 frames either".to_vec()),
        ("zstd", 4, b"zstd it is not".to_vec()),
        (
            "a gzip stream cut short",
            1,
            gzip(&encode_records(&recs(50)))[..40].to_vec(),
        ),
    ] {
        let refused = batch(codec, body, 50)
            .scan(&rules(1 << 20), &"t-0")
            .unwrap_err();
        assert_eq!(
            refused.refusal,
            Refusal::UnknownServerError,
            "{label}: {}",
            refused.reason
        );
    }
}

#[test]
fn a_zstd_frame_may_not_ask_the_decoder_for_a_window_it_would_have_to_allocate_first() {
    // Magic, a frame header descriptor with a window descriptor (not single segment), and a window of 2^(10+0x38>>3)...:
    // 0x78 is exponent 15, mantissa 0: 32 << 20 bytes, over the 8 MiB any real level declares.
    let frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x00, 0x78, 0x01, 0x00, 0x00];
    let refused = batch(4, frame, 1)
        .scan(&rules(1 << 20), &"t-0")
        .unwrap_err();
    assert_eq!(refused.refusal, Refusal::MessageTooLarge);
}

#[test]
fn a_compressed_batch_whose_records_are_not_numbered_from_zero_is_unreadable() {
    let skipping = [
        Rec {
            offset_delta: Some(0),
            ..rec_of(0)
        },
        Rec {
            offset_delta: Some(5),
            ..rec_of(1)
        },
    ];
    let refused = batch(1, gzip(&encode_records(&skipping)), 2)
        .scan(&rules(1 << 20), &"t-0")
        .unwrap_err();
    assert_eq!(
        refused.refusal,
        Refusal::UnknownServerError,
        "{}",
        refused.reason
    );
}

fn rec_of(delta: i64) -> Rec {
    Rec {
        timestamp_delta: delta,
        offset_delta: None,
        key: None,
        value: Some(vec![1]),
        headers: vec![],
    }
}

#[test]
fn bytes_after_the_last_gzip_member_that_do_not_begin_another_are_not_read() {
    let mut stream = gzip(&encode_records(&recs(10)));
    stream.extend_from_slice(b"trailing garbage, not a member");
    assert!(batch(1, stream, 10).scan(&rules(1 << 20), &"t-0").is_ok());
    // But a second member that begins and is cut off is a fault.
    let mut cut = gzip(&encode_records(&recs(10)));
    cut.extend_from_slice(&gzip(b"more")[..6]);
    assert!(batch(1, cut, 10).scan(&rules(1 << 20), &"t-0").is_err());
}
