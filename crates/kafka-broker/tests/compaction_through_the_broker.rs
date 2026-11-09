//! A compacted topic with idempotent and transactional producers, cleaned and read through the broker's handlers: what a
//! consumer sees from every offset in both isolation levels, what a restarted broker still recognises, and a transaction that
//! aborts and commits the same key.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use support::{clean_rounds, compacted, consume, from, keyed, open_on, rig, txn_produce};

#[tokio::test]
async fn one_producer_aborts_then_commits_the_same_key_and_the_old_value_is_compacted_away() {
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    assert_eq!(
        rig.produce(-1, "c", 0, keyed(&[("k", Some("old"))]))
            .await
            .error_code,
        0
    );
    let t = rig.init_txn("tp", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("tp", t, "c", &[0]).await, [0]);
    let aborted = from(t.producer, t.epoch, 0, true, &[("k", "aborted")]);
    assert_eq!(txn_produce(&rig, "tp", aborted).await.0, 0);
    assert_eq!(rig.end_txn("tp", t, false).await, 0);
    assert_eq!(rig.add_partitions("tp", t, "c", &[0]).await, [0]);
    let new = from(t.producer, t.epoch, 1, true, &[("k", "new")]);
    assert_eq!(txn_produce(&rig, "tp", new).await.0, 0);
    assert_eq!(rig.end_txn("tp", t, true).await, 0);
    for i in 0..60 {
        let one = keyed(&[(&format!("f{}", i % 4), Some("x"))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
    clean_rounds(&rig, 3).await;
    let got = consume(&rig, 0, false).await;
    let ks: Vec<_> = got.iter().filter(|(_, k)| k == "k").collect();
    assert_eq!(
        ks.len(),
        1,
        "only the committed latest value of k remains: {got:?}"
    );
}

#[tokio::test]
async fn hot_key_with_producers_and_transactions_is_read_from_every_offset_and_survives_a_restart()
{
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    let p = rig.init_producer_id(None).await;
    let (id, epoch) = (p.producer_id.0, p.producer_epoch);
    for seq in 0..5 {
        let batch = from(id, epoch, seq, false, &[("a", &format!("p{seq}"))]);
        let r = rig.produce(-1, "c", 0, batch).await;
        assert_eq!((r.error_code, r.base_offset), (0, i64::from(seq)));
    }
    // A committed and an aborted transaction, on the hot key and another.
    let t1 = rig.init_txn("t1", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("t1", t1, "c", &[0]).await, [0]);
    let committed = from(t1.producer, t1.epoch, 0, true, &[("a", "c1"), ("b", "c1")]);
    assert_eq!(txn_produce(&rig, "t1", committed).await.0, 0);
    assert_eq!(rig.end_txn("t1", t1, true).await, 0);
    let t2 = rig.init_txn("t2", 60_000).await.unwrap();
    assert_eq!(rig.add_partitions("t2", t2, "c", &[0]).await, [0]);
    let aborted = from(t2.producer, t2.epoch, 0, true, &[("ab", "x")]);
    assert_eq!(txn_produce(&rig, "t2", aborted).await.0, 0);
    assert_eq!(rig.end_txn("t2", t2, false).await, 0);
    for round in 0..4 {
        for i in 0..40 {
            let one = keyed(&[("a", Some(&format!("h{round}-{i}")))]);
            assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
        }
        clean_rounds(&rig, 2).await;
    }
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    assert_eq!(rig.list_offset(7, "c", 0, -2).await.offset, 0);
    let plain = consume(&rig, 0, false).await;
    let last = plain.last().unwrap();
    assert_eq!((last.0, last.1.as_str()), (end - 1, "a"));
    assert!(!plain.iter().any(|(_, k)| k == "ab"), "{plain:?}");
    let b: Vec<_> = plain.iter().filter(|(_, k)| k == "b").collect();
    assert_eq!(b.len(), 1, "b's committed value survives: {plain:?}");
    // Every offset can be started from, in both isolation levels, and sees the same records from there on.
    for from_offset in 0..end {
        for committed in [false, true] {
            let got = consume(&rig, from_offset, committed).await;
            let want: Vec<_> = plain
                .iter()
                .filter(|(o, _)| *o >= from_offset)
                .cloned()
                .collect();
            assert_eq!(got, want, "from {from_offset} committed={committed}");
        }
    }
    // A restart: the producer's last batch is recognised, not written twice, and the next one is accepted.
    let again = open_on(rig.fs.process_restart());
    let retry = again
        .produce(-1, "c", 0, from(id, epoch, 4, false, &[("a", "p4")]))
        .await;
    assert_eq!(
        (retry.error_code, retry.base_offset),
        (0, 4),
        "a retry of the producer's last batch after a restart"
    );
    assert_eq!(again.list_offset(7, "c", 0, -1).await.offset, end);
    let next = again
        .produce(-1, "c", 0, from(id, epoch, 5, false, &[("a", "p5")]))
        .await;
    assert_eq!(next.error_code, 0);
    // Cleaned again on the reopened broker, the log still reads through from every offset.
    for i in 0..40 {
        let one = keyed(&[("a", Some(&format!("z{i}")))]);
        assert_eq!(again.produce(-1, "c", 0, one).await.error_code, 0);
    }
    clean_rounds(&again, 3).await;
    assert_eq!(consume(&again, 0, false).await.last().unwrap().1, "a");
    let end2 = again.list_offset(7, "c", 0, -1).await.offset;
    for o in 0..end2 {
        let got = consume(&again, o, true).await;
        assert!(got.iter().all(|(oo, _)| *oo >= o));
    }
}
