//! Fetch and ListOffsets.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::time::Duration;

use support::{batch_bases, fetch_request, records, rig, topic};
use tokio::time::Instant;

#[tokio::test]
async fn a_fetch_returns_whole_batches_from_the_one_that_holds_the_offset() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    for n in [3, 2, 4] {
        rig.produce(1, "t", 0, records(n)).await;
    }
    let all = &rig.fetch_now(&[("t", 0, 0, 1 << 20)]).await[0];
    assert_eq!(
        (
            all.error_code,
            all.high_watermark,
            all.last_stable_offset,
            all.log_start_offset
        ),
        (0, 9, 9, 0)
    );
    assert_eq!(batch_bases(all.records.as_ref().unwrap()), [0, 3, 5]);
    let middle = &rig.fetch_now(&[("t", 0, 4, 1 << 20)]).await[0];
    assert_eq!(
        batch_bases(middle.records.as_ref().unwrap()),
        [3, 5],
        "from the batch that holds offset 4"
    );
    let end = &rig.fetch_now(&[("t", 0, 9, 1 << 20)]).await[0];
    assert_eq!(
        (end.error_code, end.records.as_ref().unwrap().len()),
        (0, 0),
        "caught up is an empty answer"
    );
}

#[tokio::test]
async fn an_offset_outside_the_log_is_out_of_range_with_no_offsets_and_empty_records() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(1, "t", 0, records(2)).await;
    for offset in [3, 99, -1] {
        let r = &rig.fetch_now(&[("t", 0, offset, 1 << 20)]).await[0];
        assert_eq!(r.error_code, 1, "offset {offset}");
        assert_eq!(
            (r.high_watermark, r.last_stable_offset, r.log_start_offset),
            (-1, -1, -1)
        );
        assert_eq!(r.records.as_ref().map(|b| b.len()), Some(0));
    }
}

#[tokio::test]
async fn an_unknown_topic_or_partition_is_unknown_and_by_id_it_is_an_unknown_id() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let got = rig
        .fetch_now(&[("nope", 0, 0, 100), ("t", 7, 0, 100)])
        .await;
    assert_eq!(got.iter().map(|p| p.error_code).collect::<Vec<_>>(), [3, 3]);
    let mut req = fetch_request(&[("", 0, 0, 100)], 0, 1, 1 << 20);
    req.topics[0].topic_id = kafka_wire::Uuid::from_u128(5);
    let kafka_wire::api::Response::Fetch(r) =
        rig.ask(13, kafka_wire::api::Request::Fetch(req)).await
    else {
        panic!()
    };
    assert_eq!(r.responses[0].partitions[0].error_code, 100);
}

#[tokio::test]
async fn the_requests_byte_limit_is_shared_out_and_the_first_batch_always_comes() {
    let rig = rig();
    rig.create(vec![topic("t", 2)], false).await;
    let one = records(1).len() as i32;
    for p in 0..2 {
        for _ in 0..4 {
            rig.produce(1, "t", p, records(1)).await;
        }
    }
    // Room for two batches in all: the first partition takes both, the second gets none.
    let req = fetch_request(&[("t", 0, 0, 1 << 20), ("t", 1, 0, 1 << 20)], 0, 1, one * 2);
    let got = rig.fetch(req).await;
    assert_eq!(
        got.iter()
            .map(|p| batch_bases(p.records.as_ref().unwrap()).len())
            .collect::<Vec<_>>(),
        [2, 0]
    );
    // A limit smaller than one batch still returns the first, so a large batch can always be fetched.
    let got = rig.fetch(fetch_request(&[("t", 0, 0, 1)], 0, 1, 0)).await;
    assert_eq!(batch_bases(got[0].records.as_ref().unwrap()), [0]);
    // A partition's own limit holds even where the request's is large.
    let got = rig
        .fetch(fetch_request(&[("t", 0, 0, one * 3)], 0, 1, 1 << 20))
        .await;
    assert_eq!(batch_bases(got[0].records.as_ref().unwrap()), [0, 1, 2]);
}

#[tokio::test(start_paused = true)]
async fn a_fetch_with_nothing_to_read_waits_for_a_produce_and_not_for_the_whole_time() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let started = Instant::now();
    let waiting = rig.fetch(fetch_request(&[("t", 0, 0, 1 << 20)], 5_000, 1, 1 << 20));
    let producing = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        rig.produce(1, "t", 0, records(2)).await
    };
    let (got, _) = tokio::join!(waiting, producing);
    assert_eq!(batch_bases(got[0].records.as_ref().unwrap()), [0]);
    assert_eq!(
        started.elapsed(),
        Duration::from_millis(300),
        "answered when the record arrived"
    );
}

#[tokio::test(start_paused = true)]
async fn a_fetch_that_nothing_satisfies_answers_empty_after_the_wait_and_an_error_answers_at_once()
{
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let started = Instant::now();
    let got = rig
        .fetch(fetch_request(&[("t", 0, 0, 1 << 20)], 2_000, 1, 1 << 20))
        .await;
    assert_eq!(
        (got[0].error_code, got[0].records.as_ref().unwrap().len()),
        (0, 0)
    );
    assert_eq!(started.elapsed(), Duration::from_secs(2));
    // An error is an answer: the client has to act on it, and waiting would cost it the whole time.
    let started = Instant::now();
    let got = rig
        .fetch(fetch_request(&[("t", 0, 50, 1 << 20)], 9_000, 1, 1 << 20))
        .await;
    assert_eq!(got[0].error_code, 1);
    assert_eq!(started.elapsed(), Duration::ZERO);
    // And no client can make the broker wait longer than half a minute.
    let started = Instant::now();
    rig.fetch(fetch_request(&[("t", 0, 0, 1 << 20)], i32::MAX, 1, 1 << 20))
        .await;
    assert_eq!(started.elapsed(), Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn a_fetch_waiting_on_a_topic_that_is_deleted_is_told_it_is_unknown_at_once() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let started = Instant::now();
    let waiting = rig.fetch(fetch_request(&[("t", 0, 0, 1 << 20)], 20_000, 1, 1 << 20));
    let deleting = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        rig.delete_by_name(&["t"]).await
    };
    let (got, _) = tokio::join!(waiting, deleting);
    assert_eq!(got[0].error_code, 3);
    assert_eq!(started.elapsed(), Duration::from_millis(100));
}

#[tokio::test(start_paused = true)]
async fn a_fetch_waits_until_it_has_min_bytes_and_not_a_byte_more() {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(1, "t", 0, records(1)).await;
    let size = records(1).len() as i32;
    let started = Instant::now();
    rig.fetch(fetch_request(&[("t", 0, 0, 1 << 20)], 3_000, size, 1 << 20))
        .await;
    assert_eq!(
        started.elapsed(),
        Duration::ZERO,
        "exactly min_bytes is enough"
    );
    rig.fetch(fetch_request(
        &[("t", 0, 0, 1 << 20)],
        3_000,
        size + 1,
        1 << 20,
    ))
    .await;
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(3),
        "one byte short waits out the time"
    );
}

#[tokio::test]
async fn a_read_committed_fetch_carries_an_empty_list_of_aborted_transactions_and_a_plain_one_has_none()
 {
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    rig.produce(1, "t", 0, records(1)).await;
    let mut committed = fetch_request(&[("t", 0, 0, 1 << 20)], 0, 1, 1 << 20);
    committed.isolation_level = 1;
    assert_eq!(
        rig.fetch(committed).await[0]
            .aborted_transactions
            .as_ref()
            .map(Vec::len),
        Some(0)
    );
    assert!(
        rig.fetch_now(&[("t", 0, 0, 1 << 20)]).await[0]
            .aborted_transactions
            .is_none()
    );
}

#[tokio::test]
async fn a_request_that_continues_a_fetch_session_is_told_there_is_none_and_a_full_fetch_is_not() {
    const FETCH_SESSION_ID_NOT_FOUND: i16 = 70;
    let rig = rig();
    rig.create(vec![topic("t", 1)], false).await;
    let fetch = |session_id: i32, session_epoch: i32| {
        let mut req = fetch_request(&[("t", 0, 0, 100)], 0, 1, 1 << 20);
        req.session_id = session_id;
        req.session_epoch = session_epoch;
        req
    };
    for (id, epoch, ok) in [(0, 0, true), (0, -1, true), (5, 3, false), (5, 1, false)] {
        let kafka_wire::api::Response::Fetch(r) = rig
            .ask(12, kafka_wire::api::Request::Fetch(fetch(id, epoch)))
            .await
        else {
            panic!("not Fetch")
        };
        let want = if ok { 0 } else { FETCH_SESSION_ID_NOT_FOUND };
        assert_eq!(r.error_code, want, "session {id}, epoch {epoch}");
        assert_eq!(r.session_id, 0, "every response says there is no session");
    }
}
