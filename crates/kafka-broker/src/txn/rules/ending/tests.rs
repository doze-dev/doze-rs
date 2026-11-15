use super::super::{End, Init, exhausted, expected_is_current, init};
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

/// The table of `TransactionCoordinator.endTransaction` for `TV_2`, cell by cell as the oracle answered it (`table`):
/// the current epoch asks for the next end, the one before is a retry of the end that raised it.
#[test]
fn each_end_of_the_second_transaction_version_is_answered_as_kafka_4_answers_it() {
    let at = |state, epoch: i16| {
        let mut r = rec(state);
        r.epoch = epoch;
        r
    };
    let decide = |commit| End::Decide { commit };
    let ok = End::Answer(None);
    let its = End::Answer(Some(E::InvalidTxnState));
    let fenced = End::Answer(Some(E::ProducerFenced));
    // (state, asked epoch, commit) with the record at epoch 5.
    let cells = [
        (TxnState::Empty, 5, true, its),
        (TxnState::Empty, 4, true, fenced),
        (TxnState::Empty, 4, false, fenced),
        (TxnState::Empty, 5, false, decide(false)),
        (TxnState::CompleteAbort, 4, true, its),
        (TxnState::CompleteAbort, 4, false, ok),
        (TxnState::CompleteAbort, 5, true, its),
        (TxnState::CompleteAbort, 5, false, decide(false)),
        (TxnState::CompleteAbort, 10, false, fenced),
        (TxnState::CompleteAbort, 3, false, fenced),
        (TxnState::Ongoing, 4, false, fenced),
        (TxnState::Ongoing, 6, true, fenced),
        (TxnState::Ongoing, 5, true, decide(true)),
        (TxnState::Ongoing, 5, false, decide(false)),
        (TxnState::CompleteCommit, 4, true, ok),
        (TxnState::CompleteCommit, 4, false, its),
        (TxnState::CompleteCommit, 5, true, its),
        (TxnState::CompleteCommit, 5, false, decide(false)),
        (
            TxnState::PrepareCommit,
            4,
            true,
            End::Finish {
                decided: true,
                asked: true,
            },
        ),
        (
            TxnState::PrepareAbort,
            4,
            true,
            End::Finish {
                decided: false,
                asked: true,
            },
        ),
        (TxnState::PrepareCommit, 5, true, fenced),
    ];
    for (state, epoch, commit, want) in cells {
        assert_eq!(
            end_bumping(&at(state, 5), 1000, epoch, commit),
            want,
            "{state:?} asked at {epoch}, commit {commit}"
        );
    }
    assert_eq!(
        end_bumping(&at(TxnState::CompleteAbort, 5), 2000, 5, false),
        End::Answer(Some(E::InvalidProducerIdMapping)),
        "another producer id"
    );
    assert_eq!(
        end_bumping(&TxnRecord::new("none"), 1000, 0, false),
        End::Answer(Some(E::InvalidProducerIdMapping))
    );
}

/// The oracle's `exhaust`: once a producer id's epochs are used up, the end hands over to a new producer id at 0; asked
/// again with the old id and its last epoch it is a retry, with any other epoch of the old id INVALID_PRODUCER_ID_MAPPING.
#[test]
fn an_end_that_used_up_the_epochs_hands_over_to_a_new_producer_id_and_its_retry_is_known() {
    let mut r = rec(TxnState::Ongoing);
    r.epoch = i16::MAX - 1;
    assert!(exhausted(r.epoch) && !exhausted(i16::MAX - 2));
    decide_bumping(&mut r, true, Some(2000), 7_000);
    assert_eq!(
        (
            r.state,
            r.producer_id,
            r.epoch,
            r.last_epoch,
            r.next_producer_id
        ),
        (TxnState::PrepareCommit, 1000, i16::MAX, i16::MAX - 1, 2000),
        "the markers carry the raised epoch of the old producer id"
    );
    assert_eq!(r.started_ms, 1_000, "an open transaction keeps its start");
    complete(&mut r, true);
    assert_eq!(
        (
            r.state,
            r.producer_id,
            r.prev_producer_id,
            r.epoch,
            r.last_epoch,
            r.next_producer_id
        ),
        (TxnState::CompleteCommit, 2000, 1000, 0, i16::MAX - 1, -1)
    );
    assert!(r.partitions.is_empty());
    assert_eq!(end_bumping(&r, 1000, i16::MAX - 1, true), End::Answer(None));
    assert_eq!(
        end_bumping(&r, 1000, i16::MAX - 1, false),
        End::Answer(Some(E::InvalidTxnState))
    );
    assert_eq!(
        end_bumping(&r, 1000, i16::MAX - 2, true),
        End::Answer(Some(E::InvalidProducerIdMapping))
    );
    // KIP-360 with the old id and its last epoch is answered with the new id, unbumped.
    assert_eq!(expected_is_current(&r, Some((1000, i16::MAX - 1))), Ok(()));
    assert_eq!(init(&r, Some((1000, i16::MAX - 1))), Ok(Init::Again));
}

/// A decision of the second version raises the epoch and keeps the one before; an abort of nothing begins now; the classic
/// decision keeps the epoch. The broker's own abort follows its level: at 2 as EndTxn v5, at 0 the epoch is raised and the
/// one before forgotten (so the producer it fenced cannot take the id back with it).
#[test]
fn a_decision_raises_the_epoch_at_the_second_version_and_the_brokers_own_abort_follows_its_level() {
    let mut r = rec(TxnState::CompleteCommit);
    r.last_epoch = 1;
    decide_bumping(&mut r, false, None, 9_000);
    assert_eq!(
        (
            r.state,
            r.epoch,
            r.last_epoch,
            r.started_ms,
            r.next_producer_id
        ),
        (TxnState::PrepareAbort, 3, 2, 9_000, -1)
    );
    assert_eq!(r.client_version, TxnVersion::EpochBump);
    complete(&mut r, false);
    assert_eq!(
        (r.state, r.producer_id, r.epoch),
        (TxnState::CompleteAbort, 1000, 3)
    );

    let mut c = rec(TxnState::Ongoing);
    c.client_version = TxnVersion::EpochBump;
    decide_classic(&mut c, true);
    assert_eq!(
        (c.state, c.epoch, c.client_version),
        (TxnState::PrepareCommit, 2, TxnVersion::Classic)
    );

    let mut bumped = rec(TxnState::Ongoing);
    bumped.last_epoch = 1;
    fence(&mut bumped, TxnVersion::EpochBump, None, 9_000);
    assert_eq!(
        (bumped.state, bumped.epoch, bumped.last_epoch),
        (TxnState::PrepareAbort, 3, 2)
    );
    assert_eq!(
        init(&bumped, Some((1000, 2))),
        Ok(Init::Again),
        "KIP-360 takes it back"
    );

    let mut classic = rec(TxnState::Ongoing);
    classic.last_epoch = 1;
    fence(&mut classic, TxnVersion::Classic, None, 9_000);
    assert_eq!(
        (classic.state, classic.epoch, classic.last_epoch),
        (TxnState::PrepareAbort, 3, -1)
    );
    assert_eq!(init(&classic, Some((1000, 2))), Err(E::ProducerFenced));
}
