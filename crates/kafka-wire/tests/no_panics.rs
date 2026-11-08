//! Nothing a stranger sends may panic the broker: frames, headers, request bodies of every served API at
//! every offered version, record batches and whole payloads.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::{BufMut, Bytes, BytesMut};
use kafka_wire::api::API_TABLE;
use kafka_wire::batch::{Batch, Batches};
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::incoming;
use kafka_wire::profile::{Features, KafkaMajor, Profile};
use proptest::prelude::*;

fn profiles() -> impl Iterator<Item = Profile> {
    KafkaMajor::RANGE.map(|m| Profile::new(KafkaMajor::new(m).expect("in range")))
}

const FEATURES: [Features; 2] = [
    Features {
        sasl: false,
        telemetry: false,
    },
    Features {
        sasl: true,
        telemetry: true,
    },
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..300)) {
        let frame = Bytes::from(bytes.clone());
        for profile in profiles() {
            for features in FEATURES {
                let _ = incoming::read(frame.clone(), profile, features);
            }
        }
        let _ = Batch::parse(frame.clone());
        let _ = Batch::from_produce(&frame);
        let _ = Batches::new(frame.clone()).count();
        let mut src = BytesMut::from(&bytes[..]);
        let mut d = FrameDecoder::new(Trust::Authenticated);
        while let Ok(Some(_)) = d.decode(&mut src) {}
    }

    /// A well-formed header in front of arbitrary bytes, for every served API at every version it is
    /// offered at: the decoder of each message meets hostile bodies.
    #[test]
    fn every_api_at_every_version_survives_a_hostile_body(
        body in proptest::collection::vec(any::<u8>(), 0..200)
    ) {
        for profile in profiles() {
            for adv in profile.advertised(FEATURES[1]) {
                for version in adv.min..=adv.max {
                    let mut p = BytesMut::new();
                    p.put_i16(adv.key as i16);
                    p.put_i16(version);
                    p.put_i32(1);
                    p.put_i16(-1); // no client id
                    if adv.key.request_header_version(version) >= 2 {
                        p.put_u8(0); // empty tagged fields
                    }
                    p.put_slice(&body);
                    let _ = incoming::read(p.freeze(), profile, FEATURES[1]);
                }
            }
        }
    }
}

#[test]
fn the_table_is_what_the_profiles_are_built_from() {
    // Every row is reachable from at least one profile, so no API is listed and never served.
    for row in API_TABLE {
        assert!(
            profiles().any(|p| p.range(row.key).is_some()),
            "{:?} is in no profile",
            row.key
        );
    }
}
