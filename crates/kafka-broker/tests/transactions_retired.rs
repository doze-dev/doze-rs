//! A request holding a transactional id's entry while the expiry forgets it looks the id up again, and does not write a
//! transaction the broker can no longer see.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use kafka_wire::StrBytes;
use kafka_wire::api::Request;
use kafka_wire::messages::produce_request::{PartitionProduceData, TopicProduceData};
use kafka_wire::messages::{ProduceRequest, TransactionalId};
use support::{records, rig_with, topic, wire_name};

fn plain_with_txn_id() -> ProduceRequest {
    let mut data = TopicProduceData::default();
    data.name = wire_name("t");
    let mut p = PartitionProduceData::default();
    p.index = 0;
    p.records = Some(records(1));
    data.partition_data.push(p);
    let mut req = ProduceRequest::default();
    req.acks = 1;
    req.timeout_ms = 5_000;
    req.topic_data.push(data);
    req.transactional_id = Some(TransactionalId(StrBytes::from_string("tx".into())));
    req
}

async fn queued_behind_expiry(add: bool) {
    let rig = Arc::new(rig_with(|c| {
        c.transactional_id_expiration = Duration::from_millis(100)
    }));
    rig.create(vec![topic("t", 1)], false).await;
    let old = rig.init_txn("tx", 60_000).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut others = Vec::new();
    for _ in 0..40 {
        let r = Arc::clone(&rig);
        others.push(tokio::spawn(async move {
            r.produce(1, "t", 0, records(1)).await;
        }));
    }
    let r = Arc::clone(&rig);
    let held = tokio::spawn(async move { r.ask(9, Request::Produce(plain_with_txn_id())).await });
    let r = Arc::clone(&rig);
    let sweep = tokio::spawn(async move { r.broker.check_transactions().await });
    let r = Arc::clone(&rig);
    let second = tokio::spawn(async move {
        if add {
            Err(r.add_partitions("tx", old, "t", &[0]).await)
        } else {
            Ok(r.init_txn("tx", 60_000).await.unwrap())
        }
    });
    held.await.unwrap();
    sweep.await.unwrap();
    let got = second.await.unwrap();
    for o in others {
        o.await.unwrap();
    }
    match got {
        Ok(new) => {
            // Whoever owns the id now must be able to use it.
            assert_eq!(
                rig.add_partitions("tx", new, "t", &[0]).await,
                [0],
                "new {new:?} old {old:?}"
            );
        }
        Err(codes) => {
            // The id was forgotten: the old identity is not the owner of anything.
            assert_eq!(codes, [49], "add on a forgotten id");
            let restarted = support::open_on_with(rig.fs.process_restart(), |c| {
                c.transactional_id_expiration = Duration::from_secs(3600)
            });
            restarted.broker.recover_transactions().await;
            assert_eq!(
                restarted.end_txn("tx", old, false).await,
                49,
                "the store must not have an open transaction for it"
            );
        }
    }
}

#[tokio::test]
async fn an_init_producer_id_holding_an_entry_that_expires_is_made_to_the_id_that_is_there_now() {
    queued_behind_expiry(false).await;
}

#[tokio::test]
async fn an_add_partitions_holding_an_entry_that_expires_finds_no_owner_and_leaves_nothing_in_the_store()
 {
    queued_behind_expiry(true).await;
}
