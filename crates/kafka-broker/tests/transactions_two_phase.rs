//! InitProducerId v6 (KIP-939, two-phase commit) as Kafka 4.4.0 answers it (`KafkaApis.handleInitProducerIdRequest`,
//! `TransactionCoordinator.handleInitProducerId`; docs/port/behaviours/transactions.md TX-27, docs/port/behaviours/security.md
//! SEC-336). The version is offered only with the unstable versions (kafka-wire's profile); these ask the broker directly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::{InitProducerIdRequest, InitProducerIdResponse, ProducerId, TransactionalId};
use support::acls::{LITERAL, TXN_ID, WRITE, closed_world, creation, grant, user};
use support::{Rig, rig, rig_with};

const INVALID_REQUEST: i16 = 42;
const UNSUPPORTED_VERSION: i16 = 35;
const INVALID_TRANSACTION_TIMEOUT: i16 = 50;
const TRANSACTIONAL_ID_AUTHORIZATION_FAILED: i16 = 53;
/// `AclOperation.TWO_PHASE_COMMIT`.
const TWO_PHASE_COMMIT: i8 = 15;
const ALLOW: i8 = 3;

/// An InitProducerId v6 for `id` (`None`: no transactional id) with the two flags and a timeout.
fn init(id: Option<&str>, enable_2pc: bool, keep: bool, timeout_ms: i32) -> InitProducerIdRequest {
    let mut req = InitProducerIdRequest::default();
    req.transactional_id = id.map(|t| TransactionalId(StrBytes::from_string(t.to_owned())));
    req.transaction_timeout_ms = timeout_ms;
    req.enable_2_pc = enable_2pc;
    req.keep_prepared_txn = keep;
    req
}

async fn ask(rig: &Rig, who: &Caller, req: InitProducerIdRequest) -> InitProducerIdResponse {
    let Response::InitProducerId(r) = rig.ask_as(who, 6, Request::InitProducerId(req)).await else {
        panic!("not InitProducerId")
    };
    // Kafka 4.4.0 never fills the ongoing transaction's fields: nothing keeps a prepared transaction yet.
    assert_eq!(
        (r.ongoing_txn_producer_id, r.ongoing_txn_producer_epoch),
        (ProducerId(-1), -1)
    );
    r
}

fn code_and_ids(r: &InitProducerIdResponse) -> (i16, bool) {
    (r.error_code, r.producer_id.0 >= 0 && r.producer_epoch >= 0)
}

fn two_phase(c: &mut kafka_broker::Config) {
    c.transaction_two_phase_commit = true;
}

/// With `transaction.two.phase.commit.enable` false (the default), a two-phase commit is TRANSACTIONAL_ID_AUTHORIZATION_FAILED
/// with no producer id; keeping a prepared transaction is UNSUPPORTED_VERSION whatever the setting (4.4.0 has not built it);
/// v6 without either flag is an InitProducerId like v5's.
#[tokio::test]
async fn without_the_broker_setting_a_two_phase_commit_is_refused_and_a_plain_v6_is_served() {
    let rig = rig();
    let who = support::acls::admin();
    let r = ask(&rig, &who, init(Some("tx"), true, false, 60_000)).await;
    assert_eq!((r.error_code, r.producer_id.0, r.producer_epoch), (TRANSACTIONAL_ID_AUTHORIZATION_FAILED, -1, -1));
    assert_eq!(
        code_and_ids(&ask(&rig, &who, init(Some("tx"), true, true, 60_000)).await),
        (TRANSACTIONAL_ID_AUTHORIZATION_FAILED, false),
        "the setting is asked before KeepPreparedTxn"
    );
    assert_eq!(
        code_and_ids(&ask(&rig, &who, init(Some("tx"), false, true, 60_000)).await),
        (UNSUPPORTED_VERSION, false)
    );
    assert_eq!(code_and_ids(&ask(&rig, &who, init(Some("tx"), false, false, 60_000)).await), (0, true));
    let (state, _, _) = rig.describe_txn("tx").await;
    assert_eq!(state, "Empty");
}

/// With the setting on, a two-phase commit takes the id like any InitProducerId and its transaction has no timeout: any
/// timeout asked is accepted, even one no other InitProducerId may ask (`validateTransactionTimeoutMs`), and Int.MaxValue is
/// stored; KeepPreparedTxn is still UNSUPPORTED_VERSION. A later InitProducerId that is not one stores the timeout it asks.
#[tokio::test]
async fn with_the_broker_setting_a_two_phase_commit_has_no_timeout() {
    let rig = rig_with(two_phase);
    let who = support::acls::admin();
    for timeout in [-1, 0, 60_000, 16 * 60 * 1000, i32::MAX] {
        assert_eq!(
            code_and_ids(&ask(&rig, &who, init(Some("tx"), true, false, timeout)).await),
            (0, true),
            "timeout {timeout}"
        );
        assert_eq!(rig.txn_timeout("tx").await, i32::MAX);
    }
    assert_eq!(
        code_and_ids(&ask(&rig, &who, init(Some("tx"), false, false, -1)).await),
        (INVALID_TRANSACTION_TIMEOUT, false),
        "without Enable2Pc the timeout is judged as before"
    );
    assert_eq!(
        code_and_ids(&ask(&rig, &who, init(Some("tx"), true, true, 60_000)).await),
        (UNSUPPORTED_VERSION, false)
    );
    assert_eq!(code_and_ids(&ask(&rig, &who, init(Some("tx"), false, false, 30_000)).await), (0, true));
    assert_eq!(rig.txn_timeout("tx").await, 30_000);
    // The empty id is INVALID_REQUEST before the flags are looked at, and with no id the flags are not looked at at all.
    assert_eq!(code_and_ids(&ask(&rig, &who, init(Some(""), true, true, 1)).await), (INVALID_REQUEST, false));
    let plain = rig_with(|_| {});
    for r in [&rig, &plain] {
        assert_eq!(code_and_ids(&ask(r, &who, init(None, true, true, -1)).await), (0, true));
    }
}

/// A two-phase commit of v6 takes over an id whose transaction is open as any InitProducerId does (the open one is aborted
/// and its producer fenced: KeepPreparedTxn is what would keep it).
#[tokio::test]
async fn a_two_phase_commit_fences_an_open_transaction_like_any_owner() {
    let rig = rig_with(two_phase);
    rig.create(vec![support::topic("t", 1)], false).await;
    let old = rig.init_txn("tx", 60_000).await.unwrap();
    rig.add_partitions("tx", old, "t", &[0]).await;
    rig.produce_txn("tx", old, ("t", 0), 0, 1).await;
    let r = ask(&rig, &support::acls::admin(), init(Some("tx"), true, false, 60_000)).await;
    assert_eq!(r.error_code, 0);
    assert!(r.producer_epoch > old.epoch);
    let (state, _, _) = rig.describe_txn("tx").await;
    assert_eq!(state, "Empty", "the open transaction was aborted");
    assert_eq!(rig.fetch_committed("t", 0, 0).await.aborted_transactions.unwrap().len(), 1);
}

/// The transaction of a two-phase commit is never aborted for its age: its end is the external coordinator's. One begun a
/// month ago is still open after the coordinator looks; the same with a minute's timeout is aborted.
#[tokio::test]
async fn the_transaction_of_a_two_phase_commit_never_times_out() {
    for (timeout_ms, left) in [(i32::MAX, "Ongoing"), (60_000, "CompleteAbort")] {
        let mut rec = support::owned_at("tx", 7, 3, timeout_ms);
        rec.state = kafka_meta::TxnState::Ongoing;
        rec.started_ms = rec.updated_ms - 30 * 24 * 3600 * 1000;
        let rig = support::rig_holding(&rec);
        rig.broker.check_transactions().await;
        assert_eq!(rig.describe_txn("tx").await.0, left, "timeout {timeout_ms}");
    }
}

/// Enable2Pc asks TWO_PHASE_COMMIT on the transactional id as well as WRITE, both before anything else; without Enable2Pc
/// TWO_PHASE_COMMIT is not asked. The broker setting is asked after the rights.
#[tokio::test]
async fn enable2pc_needs_two_phase_commit_on_the_id_as_well_as_write() {
    let rig = rig_with(|c| {
        closed_world(c);
        two_phase(c);
    });
    grant(
        &rig,
        vec![
            creation((TXN_ID, "tx", LITERAL), "User:alice", WRITE, ALLOW),
            creation((TXN_ID, "tx", LITERAL), "User:alice", TWO_PHASE_COMMIT, ALLOW),
            creation((TXN_ID, "tx", LITERAL), "User:bob", WRITE, ALLOW),
            creation((TXN_ID, "tx", LITERAL), "User:carol", TWO_PHASE_COMMIT, ALLOW),
        ],
    )
    .await;
    let code = |who: &str, enable| {
        let rig = &rig;
        let who = user(who);
        async move { ask(rig, &who, init(Some("tx"), enable, false, 60_000)).await.error_code }
    };
    assert_eq!(code("alice", true).await, 0);
    assert_eq!(code("bob", true).await, TRANSACTIONAL_ID_AUTHORIZATION_FAILED);
    assert_eq!(code("bob", false).await, 0);
    assert_eq!(code("carol", true).await, TRANSACTIONAL_ID_AUTHORIZATION_FAILED, "TWO_PHASE_COMMIT is not WRITE");
    assert_eq!(code("dave", true).await, TRANSACTIONAL_ID_AUTHORIZATION_FAILED);
    // KeepPreparedTxn is not a right: bob is refused by the coordinator, not by the authorizer.
    let r = ask(&rig, &user("bob"), init(Some("tx"), false, true, 60_000)).await;
    assert_eq!(r.error_code, UNSUPPORTED_VERSION);
}

impl Rig {
    /// The timeout DescribeTransactions shows for `id`.
    async fn txn_timeout(&self, id: &str) -> i32 {
        let mut req = kafka_wire::messages::DescribeTransactionsRequest::default();
        req.transactional_ids = vec![TransactionalId(StrBytes::from_string(id.to_owned()))];
        let Response::DescribeTransactions(r) = self.ask(0, Request::DescribeTransactions(req)).await else {
            panic!("not DescribeTransactions")
        };
        r.transaction_states[0].transaction_timeout_ms
    }
}
