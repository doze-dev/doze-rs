use super::*;
use crate::batch::Batch;
use crate::batch::build::{BatchBuilder, Rec, encode_records};

const NOW: i64 = 1_000_000;

fn rules() -> ScanRules {
    ScanRules {
        require_keys: false,
        log_append_time: false,
        now: TimestampMs::from_wire(NOW),
        before_max_ms: i64::MAX,
        after_max_ms: 3_600_000,
        max_inflated: 1 << 20,
        renumbering: false,
    }
}

fn rec(delta: i64) -> Rec {
    Rec {
        timestamp_delta: delta,
        offset_delta: None,
        key: Some(b"k".to_vec()),
        value: Some(b"value".to_vec()),
        headers: vec![(b"h".to_vec(), b"v".to_vec())],
    }
}

fn scan(batch: &Batch, rules: &ScanRules) -> Result<Scanned, RecordsRefused> {
    batch.scan(rules, &"t-0")
}

fn built(first: i64, recs: &[Rec]) -> Batch {
    Batch::parse(BatchBuilder::from_records(first, recs).build()).unwrap()
}

#[test]
fn the_newest_timestamp_is_the_records_and_not_what_the_header_claims() {
    let honest = BatchBuilder::from_records(NOW, &[rec(0), rec(40), rec(10)]);
    // A producer may write anything in the header; the records say 40.
    let lying = honest.timestamps(NOW, 9_999_999_999).build();
    let got = scan(&Batch::parse(lying).unwrap(), &rules()).unwrap();
    assert_eq!(got.max_timestamp.get(), NOW + 40);
}

#[test]
fn a_record_that_is_not_what_it_says_is_invalid_record() {
    let good = encode_records(&[rec(0), rec(1)]);
    let with = |body: Vec<u8>, count: u32| {
        Batch::parse(
            BatchBuilder::new()
                .records(count)
                .timestamps(NOW, NOW)
                .body(body)
                .build(),
        )
        .unwrap()
    };
    let mut cases: Vec<(&str, Batch)> = Vec::new();
    cases.push((
        "fewer records than the header counts",
        with(good.clone(), 3),
    ));
    let mut more = good.clone();
    more.push(0);
    cases.push(("bytes after the last record", with(more, 2)));
    let mut longer = good.clone();
    longer[0] += 2; // the first record says it is longer than its fields
    cases.push(("a record longer than its fields", with(longer, 2)));
    let mut shorter = good.clone();
    shorter[0] -= 2;
    cases.push(("a record shorter than its fields", with(shorter, 2)));
    cases.push((
        "a record that runs off the end",
        with(good[..good.len() - 3].to_vec(), 2),
    ));
    let mut key = vec![0u8, 0, 0];
    key.extend([0x40]); // a key of 32 bytes in a record of 6
    key.extend([0, 0]);
    let mut keyed = vec![key.len() as u8 * 2];
    keyed.extend(key);
    cases.push(("a key that runs past the record", with(keyed, 1)));
    for (label, batch) in cases {
        let refused = scan(&batch, &rules()).unwrap_err();
        assert_eq!(
            (refused.refusal, refused.before_log),
            (Refusal::InvalidRecord, false),
            "{label}: {}",
            refused.reason
        );
        assert_eq!(
            refused.message, None,
            "{label}: Kafka's answer has no text for it"
        );
    }
}

#[test]
fn a_compacted_topic_wants_a_key_on_every_record_and_says_which_in_kafkas_words() {
    let keyless = || Rec {
        key: None,
        ..rec(0)
    };
    let batch = built(NOW, &[rec(0), keyless(), keyless(), keyless(), keyless()]);
    let strict = ScanRules {
        require_keys: true,
        ..rules()
    };
    let refused = scan(&batch, &strict).unwrap_err();
    assert_eq!(refused.refusal, Refusal::InvalidRecord);
    assert_eq!(
        refused.records.iter().map(|r| r.index).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let one = "RecordError(batchIndex=1, message='Compacted topic cannot accept message without key in topic partition t-0')";
    assert!(refused.message.as_deref().unwrap().starts_with(&format!(
        "One or more records have been rejected due to 4 record errors in total, and only showing the first three errors at most: [{one}, "
    )));
    assert!(
        !refused.message.unwrap().contains("batchIndex=4"),
        "only the first three are shown"
    );
    assert!(
        scan(&batch, &rules()).is_ok(),
        "a topic that does not compact takes them"
    );
}

#[test]
fn a_create_time_record_is_held_to_the_clock_and_a_record_with_no_timestamp_is_not() {
    let narrow = ScanRules {
        before_max_ms: 100,
        after_max_ms: 50,
        ..rules()
    };
    for (delta, ok) in [
        (0, true),
        (-100, true),
        (-101, false),
        (50, true),
        (51, false),
    ] {
        let batch = built(NOW, &[rec(delta)]);
        let result = scan(&batch, &narrow);
        assert_eq!(result.is_ok(), ok, "a record {delta} ms from now");
    }
    let refused = scan(&built(NOW, &[rec(0), rec(500)]), &narrow).unwrap_err();
    assert_eq!(refused.refusal, Refusal::InvalidTimestamp);
    assert_eq!(
        refused.message.as_deref(),
        Some("One or more records have been rejected due to invalid timestamp")
    );
    assert_eq!(
        refused.records,
        [RecordError { index: 1, message: "Timestamp 1000500 of message with offset 1 is out of range. The timestamp should be within [999900, 1000050]".to_owned() }]
    );
    // No timestamp at all: the batch's records are -1 and are not judged.
    let none = BatchBuilder::from_records(-1, &[rec(0)]).build();
    assert_eq!(
        scan(&Batch::parse(none).unwrap(), &narrow)
            .unwrap()
            .max_timestamp
            .get(),
        -1
    );
}

#[test]
fn a_topic_that_stamps_does_not_hold_a_producer_to_the_clock_and_a_producer_may_not_claim_to_be_the_broker()
 {
    let stamping = ScanRules {
        log_append_time: true,
        before_max_ms: 0,
        after_max_ms: 0,
        ..rules()
    };
    assert!(
        scan(&built(0, &[rec(0)]), &stamping).is_ok(),
        "the producer's time is not looked at"
    );
    let claims = BatchBuilder::from_records(NOW, &[rec(0)])
        .attributes(0x08)
        .build();
    let refused = scan(&Batch::parse(claims).unwrap(), &stamping).unwrap_err();
    assert_eq!(refused.refusal, Refusal::InvalidTimestamp);
    assert!(
        refused.records[0]
            .message
            .contains("Producer should not set timestamp type to LogAppendTime")
    );
}

#[test]
fn offsets_inside_a_batch_may_skip_in_an_uncompressed_one() {
    let skipping = [
        Rec {
            offset_delta: Some(0),
            ..rec(0)
        },
        Rec {
            offset_delta: Some(5),
            ..rec(0)
        },
    ];
    let batch = BatchBuilder::from_records(NOW, &skipping).build();
    assert!(
        scan(&Batch::parse(batch).unwrap(), &rules()).is_ok(),
        "Kafka keeps an uncompressed one as it came"
    );
}

#[test]
fn a_hostile_number_is_refused_and_not_followed() {
    // A record length of eleven continuation bytes.
    let body = vec![0x80; 11];
    let batch = Batch::parse(
        BatchBuilder::new()
            .records(1)
            .timestamps(NOW, NOW)
            .body(body)
            .build(),
    )
    .unwrap();
    assert_eq!(
        scan(&batch, &rules()).unwrap_err().refusal,
        Refusal::InvalidRecord
    );
}
