//! Nothing a stranger sends may panic the broker: frames, headers, request bodies of every served API at
//! every offered version, record batches and whole payloads.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use bytes::{BufMut, Bytes, BytesMut};
use kafka_wire::batch::{Batch, Batches};
use kafka_wire::frame::{FrameDecoder, Trust};
use kafka_wire::incoming;
use kafka_wire::profile::{ApiSet, Features, KafkaMajor, Profile};
use proptest::prelude::*;

fn profiles() -> impl Iterator<Item = Profile> {
    KafkaMajor::RANGE.map(|m| Profile::new(KafkaMajor::new(m).expect("in range")))
}

const FEATURES: [Features; 2] = [
    Features {
        sasl: false,
        telemetry: false,
        served: ApiSet::ALL,
    },
    Features {
        sasl: true,
        telemetry: true,
        served: ApiSet::ALL,
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
        let _ = Batch::from_produce(&frame, kafka_wire::batch::ProduceRules { version: 9, max_batch_bytes: 1 << 20 });
        let _ = Batches::new(frame.clone()).count();
        let mut src = BytesMut::from(&bytes[..]);
        let mut d = FrameDecoder::new(Trust::Authenticated);
        while let Ok(Some(_)) = d.decode(&mut src) {}
    }

    /// A valid batch header in front of arbitrary bytes as its records, under every codec: reading the records
    /// (and walking them for their times) is a parser of whatever a producer sends.
    #[test]
    fn arbitrary_records_never_panic_the_scan(
        body in proptest::collection::vec(any::<u8>(), 0..300),
        codec in 0u16..=4,
        count in 1u32..8,
    ) {
        use kafka_wire::batch::build::BatchBuilder;
        use kafka_wire::batch::ScanRules;
        use kafka_wire::ids::TimestampMs;
        let built = BatchBuilder::new().records(count).attributes(codec).body(body).build();
        if let Ok(batch) = Batch::parse(built) {
            let rules = ScanRules {
                require_keys: false,
                log_append_time: false,
                now: TimestampMs::from_wire(1_000),
                before_max_ms: i64::MAX,
                after_max_ms: 1_000,
                max_inflated: 1 << 16,
                renumbering: false,
            };
            let _ = batch.scan(&rules, &"t-0");
            let _ = batch.record_times().map(Iterator::count);
        }
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
