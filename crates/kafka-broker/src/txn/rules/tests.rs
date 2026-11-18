use super::*;
use kafka_meta::TxnVersion;

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
    assert_eq!(may_write(&r, 1000, 2, ("t", 0), 9), Ok(()));
    assert_eq!(
        may_write(&r, 1000, 2, ("t", 1), 9),
        Err(E::InvalidTxnState),
        "the partition was not added"
    );
    assert_eq!(
        may_write(&r, 1000, 1, ("t", 0), 9),
        Err(E::InvalidProducerEpoch),
        "fenced is reported as the epoch"
    );
    assert_eq!(
        may_write(&r, 7, 2, ("t", 0), 9),
        Err(E::InvalidProducerIdMapping)
    );
    assert_eq!(
        may_write(&rec(TxnState::Empty), 1000, 2, ("t", 0), 9),
        Err(E::InvalidTxnState),
        "no transaction is open"
    );
}

/// KIP-890's first part, as Kafka 4.4.0 answered the oracle (`unadded`): a write to a partition the transaction does not
/// have is INVALID_TXN_STATE up to Produce v10 and TRANSACTION_ABORTABLE from v11; a transaction being ended is
/// NOT_ENOUGH_REPLICAS (retriable for every client) before v12 and CONCURRENT_TRANSACTIONS from it.
#[test]
fn an_unverified_write_is_refused_with_the_code_its_version_knows() {
    let r = rec(TxnState::Ongoing);
    for (version, code) in [
        (3, E::InvalidTxnState),
        (10, E::InvalidTxnState),
        (11, E::TransactionAbortable),
        (12, E::TransactionAbortable),
    ] {
        assert_eq!(
            may_write(&r, 1000, 2, ("t", 1), version),
            Err(code),
            "v{version}"
        );
        assert_eq!(
            may_write(&rec(TxnState::CompleteCommit), 1000, 2, ("t", 0), version),
            Err(code),
            "v{version}: nothing open"
        );
    }
    for state in [TxnState::PrepareCommit, TxnState::PrepareAbort] {
        assert_eq!(
            may_write(&rec(state), 1000, 2, ("t", 0), 11),
            Err(E::NotEnoughReplicas)
        );
        assert_eq!(
            may_write(&rec(state), 1000, 2, ("t", 0), 12),
            Err(E::ConcurrentTransactions)
        );
    }
    assert_eq!(not_added(3, 4), E::InvalidTxnState);
    assert_eq!(not_added(4, 4), E::TransactionAbortable);
}

/// What a request to add is told before anything is stored (`handleAddPartitionsToTransaction`).
#[test]
fn an_add_is_refused_for_another_owner_or_an_ending_and_is_nothing_when_all_is_there() {
    let r = rec(TxnState::Ongoing);
    assert_eq!(may_add(&r, 1000, 2, true), Adding::There);
    assert_eq!(may_add(&r, 1000, 2, false), Adding::Store);
    assert_eq!(
        may_add(&rec(TxnState::CompleteAbort), 1000, 2, true),
        Adding::Store
    );
    assert_eq!(
        may_add(&r, 1000, 3, true),
        Adding::Refused(E::ProducerFenced)
    );
    assert_eq!(
        may_add(&r, 7, 2, true),
        Adding::Refused(E::InvalidProducerIdMapping)
    );
    assert_eq!(
        may_add(&rec(TxnState::PrepareCommit), 1000, 2, true),
        Adding::Refused(E::ConcurrentTransactions)
    );
    let mut begun = rec(TxnState::CompleteCommit);
    add(
        &mut begun,
        [("u".to_owned(), 3)],
        TxnVersion::EpochBump,
        8_000,
    );
    assert_eq!(
        (
            begun.state,
            begun.started_ms,
            begun.partitions.clone(),
            begun.client_version
        ),
        (
            TxnState::Ongoing,
            8_000,
            vec![("u".to_owned(), 3)],
            TxnVersion::EpochBump
        )
    );
}

#[test]
fn a_producer_says_the_producer_id_and_epoch_it_holds_and_is_fenced_if_they_are_not_the_ids() {
    let mut r = rec(TxnState::CompleteCommit);
    r.last_epoch = 1;
    // Nobody says: a new owner takes the id and its epoch goes up.
    assert_eq!(init(&r, None), Ok(Init::Bump { last: -1 }));
    // The owner says what it holds: the next epoch, and the one it had is kept.
    assert_eq!(expected_is_current(&r, Some((1000, 2))), Ok(()));
    assert_eq!(init(&r, Some((1000, 2))), Ok(Init::Bump { last: 2 }));
    // It asks again with the epoch before the bump, because the answer was lost: the same answer.
    assert_eq!(init(&r, Some((1000, 1))), Ok(Init::Again));
    // Any other epoch is a producer that has been replaced.
    assert_eq!(init(&r, Some((1000, 0))), Err(E::ProducerFenced));
    assert_eq!(init(&r, Some((1000, 3))), Err(E::ProducerFenced));
    // Another producer id is not the owner's.
    assert_eq!(
        expected_is_current(&r, Some((999, 2))),
        Err(E::ProducerFenced)
    );
    // Except that a record that has only just been made has no owner to fence.
    assert_eq!(
        expected_is_current(&TxnRecord::new("x"), Some((5, 3))),
        Ok(())
    );
    assert_eq!(
        init(&TxnRecord::new("x"), Some((5, 3))),
        Ok(Init::Bump { last: -1 })
    );
}

#[test]
fn when_the_epochs_are_used_up_the_producer_id_is_replaced_and_the_old_one_is_still_known_for_a_retry()
 {
    let mut r = rec(TxnState::CompleteAbort);
    r.epoch = i16::MAX - 1;
    assert_eq!(init(&r, None), Ok(Init::Rotate { last: -1 }));
    assert_eq!(
        init(&r, Some((1000, i16::MAX - 1))),
        Ok(Init::Rotate { last: i16::MAX - 1 })
    );
    // After the rotation the old id with its last epoch is a retry of the rotation, not a stranger.
    r.producer_id = 2000;
    r.epoch = 0;
    r.prev_producer_id = 1000;
    r.last_epoch = i16::MAX - 1;
    assert_eq!(expected_is_current(&r, Some((1000, i16::MAX - 1))), Ok(()));
    assert_eq!(init(&r, Some((1000, i16::MAX - 1))), Ok(Init::Again));
    assert_eq!(
        expected_is_current(&r, Some((1000, 3))),
        Err(E::ProducerFenced)
    );
}

#[test]
fn the_next_step_is_the_first_moment_the_timeout_or_the_expiry_holds() {
    let limit = 7_000;
    let open = rec(TxnState::Ongoing);
    let at = next_step_ms(&open, limit).unwrap();
    assert_eq!(at, 61_001);
    assert!(
        !expired(&open, at - 1) && expired(&open, at),
        "exactly then"
    );
    for state in [
        TxnState::Empty,
        TxnState::CompleteCommit,
        TxnState::CompleteAbort,
    ] {
        let r = rec(state);
        let at = next_step_ms(&r, limit).unwrap();
        assert!(!idle(&r, at - 1, limit) && idle(&r, at, limit), "{state:?}");
    }
    assert_eq!(next_step_ms(&rec(TxnState::PrepareCommit), limit), None);
    assert_eq!(next_step_ms(&rec(TxnState::PrepareAbort), limit), None);
}

/// KIP-939: a two-phase commit accepts any timeout and stores Int.MaxValue (`validateTransactionTimeoutMs`), and a transaction
/// with it is never aborted for its age nor woken for it (`isDistributedTwoPhaseCommitTxn`); any other timeout is held to
/// 1 ms to 15 minutes.
#[test]
fn a_two_phase_commit_has_no_timeout_and_never_expires() {
    assert_eq!(Timeout::TwoPhase.to_store(), Ok(i32::MAX));
    assert_eq!(Timeout::Ms(1).to_store(), Ok(1));
    assert_eq!(Timeout::Ms(MOST_TIMEOUT_MS).to_store(), Ok(MOST_TIMEOUT_MS));
    for bad in [i32::MIN, -1, 0, MOST_TIMEOUT_MS + 1, i32::MAX] {
        assert_eq!(Timeout::Ms(bad).to_store(), Err(E::InvalidTransactionTimeout), "{bad}");
    }
    let mut r = rec(TxnState::Ongoing);
    r.timeout_ms = TWO_PHASE_TIMEOUT_MS;
    assert!(!expired(&r, i64::MAX), "never over it");
    assert_eq!(next_step_ms(&r, 7 * 24 * 3600 * 1000), None, "nothing to wake for");
    r.timeout_ms = TWO_PHASE_TIMEOUT_MS - 1;
    assert!(expired(&r, i64::MAX), "one millisecond less is an ordinary timeout");
    assert!(next_step_ms(&r, 0).is_some());
}
