//! Reading a log that compaction has left batches with no records in.

use kafka_wire::batch::Batches;

use super::compaction::{keyed, plain};
use crate::testutil::offset;

#[test]
fn a_read_that_lands_on_a_batch_with_no_records_goes_on_to_the_records_after_it_unless_nothing_may_follow()
 {
    let mut log = keyed(&[
        ("a", Some("1")),
        ("a", Some("2")),
        ("a", Some("3")),
        ("a", Some("4")),
        ("a", Some("5")),
        ("a", Some("6")),
        ("b", Some("1")),
    ]);
    plain(&mut log).unwrap();
    let first_batch = |bytes: bytes::Bytes| Batches::new(bytes).next().unwrap().unwrap();
    // Offsets 0 to 2 are an empty batch (at 2) and then comes the segment that holds 5.
    let read = log.reader().read(offset(1), 1 << 20, None).unwrap();
    let first = first_batch(read);
    assert_eq!(
        (first.base_offset(), first.record_count()),
        (Some(offset(5)), 1)
    );
    // With a limit short of what follows, the empty batch is what there is, so that the reader moves on past it.
    let read = log
        .reader()
        .read(offset(1), 1 << 20, Some(offset(3)))
        .unwrap();
    let first = first_batch(read);
    assert_eq!(
        (first.base_offset(), first.record_count()),
        (Some(offset(2)), 0)
    );
}
