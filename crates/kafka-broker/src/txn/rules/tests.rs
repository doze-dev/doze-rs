use super::*;

fn rec(state: TxnState) -> TxnRecord {
    let mut r = TxnRecord::new("tx");
    r.producer_id = 1000;
    r.epoch = 2;
    r.timeout_ms = 60_000;
    r.state = state;
    r.started_ms = 1_000;
    r.updated_ms = 50_000;
    r.partitions = vec![("t".into(), 0)];
    r
}

#[test]
fn the_owner_is_the_producer_id_and_epoch_on_the_record_and_a_lower_epoch_is_fenced() {
    let r = rec(TxnState::Ongoing);
    assert_eq!(owns(&r, 1000, 2), None);
    assert_eq!(owns(&r, 999, 2), Some(E::InvalidProducerIdMapping));
    assert_eq!(owns(&r, 1000, 1), Some(E::ProducerFenced));
    assert_eq!(
        owns(&r, 1000, 3),
        Some(E::ProducerFenced),
        "a higher epoch is fenced too"
    );
    assert_eq!(fenced_as(E::ProducerFenced, 1, 2), E::InvalidProducerEpoch);
    assert_eq!(fenced_as(E::ProducerFenced, 2, 2), E::ProducerFenced);
    assert_eq!(fenced_as(E::InvalidTxnState, 0, 2), E::InvalidTxnState);
    assert_eq!(
        owns(&TxnRecord::new("none"), 0, 0),
        Some(E::InvalidProducerIdMapping),
        "nobody was ever given the id"
    );
}

#[test]
fn nothing_is_added_while_a_transaction_is_being_ended_and_begin_starts_afresh() {
    assert_eq!(
        can_add(&rec(TxnState::PrepareCommit)),
        Some(E::ConcurrentTransactions)
    );
    assert_eq!(
        can_add(&rec(TxnState::PrepareAbort)),
        Some(E::ConcurrentTransactions)
    );
    assert_eq!(can_add(&rec(TxnState::Ongoing)), None);
    let mut r = rec(TxnState::CompleteCommit);
    r.groups = vec!["g".into()];
    begin(&mut r, 5_000);
    assert_eq!(
        (r.state, r.started_ms, r.partitions.len(), r.groups.len()),
        (TxnState::Ongoing, 5_000, 0, 0)
    );
    r.partitions.push(("t".into(), 1));
    begin(&mut r, 9_000);
    assert_eq!(
        (r.started_ms, r.partitions.len()),
        (5_000, 1),
        "an open transaction is not started again"
    );
}

#[test]
fn each_end_is_answered_as_the_recording_says() {
    use End::*;
    assert_eq!(end(&rec(TxnState::Ongoing), true), Decide { commit: true });
    assert_eq!(
        end(&rec(TxnState::Ongoing), false),
        Decide { commit: false }
    );
    assert_eq!(
        end(&rec(TxnState::PrepareCommit), false),
        Finish {
            decided: true,
            asked: false
        }
    );
    assert_eq!(
        end(&rec(TxnState::PrepareAbort), false),
        Finish {
            decided: false,
            asked: false
        }
    );
    assert_eq!(
        end(&rec(TxnState::CompleteCommit), true),
        Answer(None),
        "a retry of an end that succeeded"
    );
    assert_eq!(
        end(&rec(TxnState::CompleteCommit), false),
        Answer(Some(E::InvalidTxnState))
    );
    assert_eq!(end(&rec(TxnState::CompleteAbort), false), Answer(None));
    assert_eq!(
        end(&rec(TxnState::CompleteAbort), true),
        Answer(Some(E::InvalidTxnState))
    );
    assert_eq!(
        end(&rec(TxnState::Empty), true),
        Answer(Some(E::InvalidTxnState))
    );
}

#[test]
fn a_transaction_outlives_its_timeout_from_when_it_began_not_from_its_last_change() {
    let r = rec(TxnState::Ongoing);
    assert!(!expired(&r, 61_000), "exactly the timeout is not over it");
    assert!(expired(&r, 61_001));
    assert!(
        !expired(&rec(TxnState::Empty), 10_000_000),
        "only an open transaction times out"
    );
}

#[test]
fn a_batch_may_be_written_only_by_the_owner_to_a_partition_of_the_open_transaction() {
    let r = rec(TxnState::Ongoing);
    assert_eq!(may_write(&r, 1000, 2, "t", 0), Ok(()));
    assert_eq!(
        may_write(&r, 1000, 2, "t", 1),
        Err(E::InvalidTxnState),
        "the partition was not added"
    );
    assert_eq!(
        may_write(&r, 1000, 1, "t", 0),
        Err(E::InvalidProducerEpoch),
        "fenced is reported as the epoch"
    );
    assert_eq!(
        may_write(&r, 7, 2, "t", 0),
        Err(E::InvalidProducerIdMapping)
    );
    assert_eq!(
        may_write(&rec(TxnState::Empty), 1000, 2, "t", 0),
        Err(E::InvalidTxnState),
        "no transaction is open"
    );
}
