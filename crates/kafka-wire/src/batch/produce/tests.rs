use super::*;
use crate::batch::build::{BatchBuilder, reseal};

fn one() -> Vec<u8> {
    BatchBuilder::new().records(2).build().to_vec()
}

const RULES: ProduceRules = ProduceRules {
    version: 9,
    max_batch_bytes: 1 << 20,
};

fn from(bytes: Vec<u8>) -> Result<Batch, ProduceError> {
    Batch::from_produce(&Bytes::from(bytes), RULES)
}

fn refusal(bytes: Vec<u8>) -> Refusal {
    from(bytes).unwrap_err().refusal()
}

#[test]
fn one_batch_is_taken() {
    assert_eq!(from(one()).unwrap().record_count(), 2);
}

#[test]
fn nothing_whole_is_invalid_record() {
    assert_eq!(from(vec![]).unwrap_err(), ProduceError::NoWholeBatch(0));
    // A batch that says it is longer than what was sent.
    let mut cut = one();
    cut.pop();
    assert!(matches!(
        from(cut).unwrap_err(),
        ProduceError::NoWholeBatch(_)
    ));
    assert_eq!(refusal(vec![0; 10]), Refusal::InvalidRecord);
}

#[test]
fn a_second_whole_batch_is_invalid_record() {
    let mut two = one();
    two.extend_from_slice(&one());
    assert_eq!(from(two.clone()).unwrap_err(), ProduceError::MoreThanOne);
    assert_eq!(refusal(two), Refusal::InvalidRecord);
}

#[test]
fn bytes_after_the_batch_that_are_not_a_whole_batch_are_dropped() {
    let mut stray = one();
    stray.extend_from_slice(&[1, 2, 3]);
    assert_eq!(from(stray).unwrap().len(), one().len());
    // A second batch cut short is dropped too.
    let mut partial = one();
    let mut second = one();
    second.pop();
    partial.extend_from_slice(&second);
    assert_eq!(from(partial).unwrap().len(), one().len());
}

#[test]
fn an_impossible_size_or_format_byte_is_corrupt() {
    let mut small = one();
    small[8..12].copy_from_slice(&13i32.to_be_bytes());
    assert_eq!(
        from(small.clone()).unwrap_err(),
        ProduceError::DeclaredSize(13)
    );
    assert_eq!(refusal(small), Refusal::CorruptMessage);
    let mut odd = one();
    odd[16] = 7;
    assert_eq!(
        from(odd.clone()).unwrap_err(),
        ProduceError::UnknownFormat(7)
    );
    assert_eq!(refusal(odd), Refusal::CorruptMessage);
    // The same fault in the bytes after the first batch is the same refusal.
    let mut after = one();
    let mut second = one();
    second[8..12].copy_from_slice(&1i32.to_be_bytes());
    after.extend_from_slice(&second);
    assert_eq!(refusal(after), Refusal::CorruptMessage);
}

#[test]
fn an_old_format_is_invalid_record() {
    let mut old = one();
    old[16] = 1;
    assert_eq!(from(old).unwrap_err(), ProduceError::OldFormat(1));
}

#[test]
fn a_base_offset_other_than_zero_is_invalid_record() {
    let mut v = BatchBuilder::new().base_offset(5).build().to_vec();
    assert_eq!(from(v.clone()).unwrap_err(), ProduceError::BaseOffset(5));
    v = BatchBuilder::new().base_offset(-1).build().to_vec();
    assert_eq!(from(v).unwrap_err(), ProduceError::BaseOffset(-1));
}

#[test]
fn a_bad_batch_is_answered_by_its_own_fault() {
    let mut crc = one();
    let last = crc.len() - 1;
    crc[last] ^= 1;
    assert_eq!(refusal(crc), Refusal::CorruptMessage);
    let codec = BatchBuilder::new().attributes(5).build().to_vec();
    assert_eq!(refusal(codec), Refusal::UnknownServerError);
    let mut count = one();
    count[23..27].copy_from_slice(&9i32.to_be_bytes());
    reseal(&mut count);
    assert_eq!(refusal(count), Refusal::InvalidRecord);
    // Shorter than a header, though it declares a plausible size.
    let mut short = one();
    short.truncate(40);
    short[8..12].copy_from_slice(&28i32.to_be_bytes());
    assert_eq!(refusal(short), Refusal::CorruptMessage);
}

#[test]
fn clients_may_not_write_control_batches() {
    let control = BatchBuilder::new().attributes(0x20).build().to_vec();
    assert_eq!(from(control).unwrap_err(), ProduceError::ControlBatch);
}

#[test]
fn a_producer_id_needs_a_sequence() {
    let v = BatchBuilder::new().producer(9, 0, -1).build().to_vec();
    assert_eq!(from(v).unwrap_err(), ProduceError::NoSequence(9));
    assert!(from(BatchBuilder::new().producer(9, 0, 0).build().to_vec()).is_ok());
}

#[test]
fn the_request_layer_can_ask_whether_a_first_batch_is_transactional() {
    let tx = |b: Vec<u8>| Batch::first_is_transactional(&Bytes::from(b));
    assert_eq!(tx(one()), Ok(false));
    assert_eq!(
        tx(BatchBuilder::new().attributes(0x10).build().to_vec()),
        Ok(true)
    );
    assert_eq!(tx(vec![]), Ok(false));
    let mut small = one();
    small[8..12].copy_from_slice(&1i32.to_be_bytes());
    assert_eq!(tx(small), Err(ProduceError::DeclaredSize(1)));
}

#[test]
fn zstd_needs_produce_v7_and_is_judged_before_what_follows_the_first_batch() {
    // Attribute bits 0-2 are the codec; 4 is zstd. The body is not a zstd stream, which the checks that
    // would read it come after.
    let zstd = BatchBuilder::new()
        .records(1)
        .attributes(4)
        .build()
        .to_vec();
    let old = Batch::from_produce(
        &Bytes::from(zstd.clone()),
        ProduceRules {
            version: 6,
            ..RULES
        },
    );
    assert_eq!(old.unwrap_err(), ProduceError::Zstd(6));
    assert_eq!(
        ProduceError::Zstd(6).refusal(),
        Refusal::UnsupportedCompression
    );
    assert!(
        Batch::from_produce(
            &Bytes::from(zstd),
            ProduceRules {
                version: 7,
                ..RULES
            }
        )
        .is_ok()
    );
}

#[test]
fn a_batch_over_the_limit_is_too_large_whatever_else_is_wrong_with_it() {
    let mut bad_crc = one();
    let last = bad_crc.len() - 1;
    bad_crc[last] ^= 0xff; // the checksum no longer matches, which is judged after the size
    let tiny = ProduceRules {
        max_batch_bytes: 10,
        ..RULES
    };
    let err = Batch::from_produce(&Bytes::from(bad_crc.clone()), tiny).unwrap_err();
    assert!(matches!(err, ProduceError::TooLarge { limit: 10, .. }));
    assert_eq!(err.refusal(), Refusal::MessageTooLarge);
    // Within the limit it is the checksum.
    assert_eq!(refusal(bad_crc), Refusal::CorruptMessage);
}

#[test]
fn kafka_words_a_batch_that_is_not_a_batch_in_its_own_text() {
    let mut size = one();
    size[8..12].copy_from_slice(&5i32.to_be_bytes());
    assert_eq!(
        from(size).unwrap_err().kafka_message().as_deref(),
        Some("Record size 5 is less than the minimum record overhead (14)")
    );
    let mut magic = one();
    magic[16] = 9;
    assert_eq!(
        from(magic).unwrap_err().kafka_message().as_deref(),
        Some("Invalid magic found in record: 9")
    );
    assert_eq!(ProduceError::MoreThanOne.kafka_message(), None);
}
