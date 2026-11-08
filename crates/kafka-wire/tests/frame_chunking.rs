//! Bytes arrive in pieces. However the stream is cut, the same frames come out.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::{Bytes, BytesMut};
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::testing::framed;
use proptest::prelude::*;

/// Frames of ApiVersions-shaped payloads (key 18, version 0) with a body of `n` bytes.
fn payloads() -> impl Strategy<Value = Vec<Vec<u8>>> {
    proptest::collection::vec(
        proptest::collection::vec(any::<u8>(), 0..64).prop_map(|body| {
            let mut p = vec![0, 18, 0, 0];
            p.extend(body);
            p
        }),
        1..6,
    )
}

fn drain(d: &mut FrameDecoder, src: &mut BytesMut, out: &mut Vec<Bytes>) {
    while let Ok(Some(f)) = d.decode(src) {
        out.push(f);
    }
}

proptest! {
    #[test]
    fn any_split_gives_the_same_frames(payloads in payloads(), cuts in proptest::collection::vec(1usize..40, 1..30)) {
        let mut stream = Vec::new();
        for p in &payloads {
            stream.extend_from_slice(&framed(p));
        }

        let mut whole = Vec::new();
        drain(&mut FrameDecoder::new(Trust::Authenticated), &mut BytesMut::from(&stream[..]), &mut whole);

        let mut pieces = Vec::new();
        let (mut d, mut src, mut at) = (FrameDecoder::new(Trust::Authenticated), BytesMut::new(), 0usize);
        for cut in cuts.iter().cycle() {
            if at >= stream.len() { break; }
            let end = (at + cut).min(stream.len());
            src.extend_from_slice(&stream[at..end]);
            at = end;
            drain(&mut d, &mut src, &mut pieces);
        }

        prop_assert_eq!(&whole, &pieces);
        prop_assert_eq!(whole.len(), payloads.len());
        for (frame, payload) in whole.iter().zip(&payloads) {
            prop_assert_eq!(&frame[..], &payload[..]);
        }
    }
}
