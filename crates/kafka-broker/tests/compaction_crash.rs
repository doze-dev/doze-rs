//! A stop at every file operation of a broker's round of cleaning, by a kill and by a power cut: the latest record of every
//! key is still served at its offset, the topic takes writes, and the cleaning run again finishes the work.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::{Cut, DataCut, NameCut};
use kafka_wire::batch::Batches;
use support::{keyed, open_on, rig, topic, with_config};

async fn world() -> support::Rig {
    let t = with_config(topic("c", 1), "cleanup.policy", "compact");
    let t = with_config(t, "segment.bytes", "1024");
    let t = with_config(t, "min.cleanable.dirty.ratio", "0.01");
    let rig = rig();
    rig.create(vec![t], false).await;
    for i in 0..60 {
        let one = keyed(&[(&format!("k{}", i % 5), Some(&format!("v{i}")))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
    rig
}

async fn served(rig: &support::Rig, expected_end: i64) -> Vec<(i64, String)> {
    let end = rig.list_offset(7, "c", 0, -1).await.offset;
    assert_eq!(end, expected_end, "the end does not move");
    let (mut at, mut out) = (0, Vec::new());
    while at < end {
        let data = rig.fetch_now(&[("c", 0, at, 1 << 20)]).await.remove(0);
        assert_eq!(data.error_code, 0, "fetch from {at}");
        for b in Batches::new(data.records.unwrap()) {
            let b = b.unwrap();
            b.each_record(1 << 20, |s| {
                if s.offset >= at {
                    out.push((
                        s.offset,
                        String::from_utf8(s.key.unwrap().to_vec()).unwrap(),
                    ));
                }
            })
            .unwrap();
            at = b.next_offset().unwrap().get();
        }
    }
    out
}

fn holds(seen: &[(i64, String)], what: &str) {
    for k in 0..5 {
        // The latest write of k is the last i < 60 with i % 5 == k.
        let latest = 55 + k;
        assert!(
            seen.contains(&(latest, format!("k{k}"))),
            "{what}: the latest of k{k} (offset {latest}) is gone: {seen:?}"
        );
    }
    assert!(
        seen.windows(2).all(|w| w[0].0 < w[1].0),
        "{what}: out of order"
    );
}

#[tokio::test]
async fn a_stop_at_any_file_operation_of_a_round_loses_nothing_and_the_round_run_again_finishes() {
    let clean = world().await;
    let before = clean.fs.ops();
    clean.broker.check_cleaner().await;
    let steps = clean.fs.ops() - before;
    let want = served(&clean, 60).await;
    assert!(
        want.len() < 60 && steps > 10,
        "{} left, {steps} steps",
        want.len()
    );
    let ends = |stop| {
        let mut all = vec![None];
        for names in [NameCut::Durable, NameCut::Volatile, NameCut::Random] {
            all.push(Some(Cut {
                data: DataCut::Drop,
                names,
                seed: stop,
            }));
        }
        all
    };
    for stop in 0..=steps {
        for end in ends(stop) {
            let rig = world().await;
            rig.fs.halt_after(stop);
            rig.broker.check_cleaner().await;
            let disk = end.map_or_else(|| rig.fs.process_restart(), |cut| rig.fs.power_cut(cut));
            let what = format!("stop {stop}/{steps}, {end:?}");
            let again = open_on(disk);
            holds(&served(&again, 60).await, &what);
            let one = keyed(&[("k9", Some("after"))]);
            assert_eq!(
                again.produce(-1, "c", 0, one).await.error_code,
                0,
                "{what}: takes writes"
            );
            again.broker.check_cleaner().await;
            let finished = served(&again, 61).await;
            holds(&finished, &format!("{what}, cleaned again"));
        }
    }
}
