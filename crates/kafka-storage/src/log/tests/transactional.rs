//! The note that a log holds transactions: made before the first, durable, and absent from a log that never had one.

use crate::sim::{Cut, DataCut, NameCut};

use super::{NEVER, fresh, open};
use crate::log::Trust;

#[test]
fn a_log_that_was_never_told_of_a_transaction_holds_none_and_one_that_was_always_says_so() {
    let (fs, dir, mut log) = fresh(NEVER);
    assert!(!log.holds_transactions());
    log.note_transactions().unwrap();
    assert!(log.holds_transactions());
    // Noting again changes nothing (and costs nothing).
    let ops = fs.ops();
    log.note_transactions().unwrap();
    assert_eq!(fs.ops(), ops);
    drop(log);
    // A killed process, and a power cut that takes what was not flushed: the note is still there.
    let killed = open(&fs.process_restart(), &dir, NEVER, Trust::Unvouched);
    assert!(killed.holds_transactions());
    let cut = fs.power_cut(Cut {
        data: DataCut::Drop,
        names: NameCut::Durable,
        seed: 3,
    });
    assert!(open(&cut, &dir, NEVER, Trust::Unvouched).holds_transactions());
}

#[test]
fn a_failed_note_takes_the_log_offline_and_is_not_taken_for_made() {
    let (fs, _dir, mut log) = fresh(NEVER);
    fs.fail_any(1, crate::sim::Failure::Io);
    assert!(log.note_transactions().is_err());
    assert!(!log.holds_transactions());
}
