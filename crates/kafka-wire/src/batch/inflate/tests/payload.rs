//! A telemetry payload (KIP-714) read to its end through a fixed buffer, under every codec.

use super::{gzip, lz4, xerial, zstd};

#[test]
fn a_payload_is_read_to_its_end_under_every_codec_and_the_limit_is_exact() {
    use crate::batch::Compression::{Gzip, Lz4, None, Snappy, Unknown, Zstd};
    use crate::batch::inflates_within;
    let plain = vec![7_u8; 5000];
    let raw_snappy = snap::raw::Encoder::new().compress_vec(&plain).unwrap();
    let packed = [
        (Gzip, gzip(&plain)),
        (Snappy, raw_snappy),
        (Snappy, xerial(&plain, 1000)),
        (Lz4, lz4(&plain)),
        (Zstd, zstd(&plain)),
    ];
    for (codec, bytes) in &packed {
        assert!(
            inflates_within(*codec, bytes, 5000),
            "{codec:?} at the limit"
        );
        assert!(!inflates_within(*codec, bytes, 4999), "{codec:?} one under");
        assert!(
            !inflates_within(*codec, &bytes[..bytes.len() / 2], 5000),
            "{codec:?} cut short"
        );
    }
    // Not compressed: its own length, held to the limit by the caller's own check as well.
    assert!(inflates_within(None, &plain, 5000) && !inflates_within(None, &plain, 4999));
    // A codec this broker does not know cannot be read.
    assert!(!inflates_within(Unknown(6), &plain, u64::MAX));
}

#[test]
fn a_payload_that_claims_far_more_than_it_weighs_is_refused_at_the_limit_not_inflated() {
    use crate::batch::Compression::{Gzip, Zstd};
    use crate::batch::inflates_within;
    let zeros = vec![0_u8; 64 << 20];
    let bomb = gzip(&zeros);
    assert!(bomb.len() < 1 << 16, "{} bytes", bomb.len());
    assert!(!inflates_within(Gzip, &bomb, 1 << 20));
    assert!(inflates_within(Gzip, &bomb, 64 << 20));
    assert!(!inflates_within(Zstd, &zstd(&zeros), 1 << 20));
}

#[test]
fn bytes_that_are_not_a_stream_are_refused_under_every_codec() {
    use crate::batch::Compression::{Gzip, Lz4, Snappy, Zstd};
    use crate::batch::inflates_within;
    for codec in [Gzip, Snappy, Lz4, Zstd] {
        for junk in [
            &b""[..],
            b"this is not a compressed body",
            &[0xff; 64],
            &[0; 64],
        ] {
            assert!(!inflates_within(codec, junk, 1 << 20), "{codec:?} {junk:?}");
        }
    }
}
