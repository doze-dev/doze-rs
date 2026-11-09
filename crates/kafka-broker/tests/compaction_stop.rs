//! A broker that is stopping tells the round of cleaning it may be running to give up, so that the stop does not wait for the
//! end of a round of a large log and no half-made file is left behind.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_storage::sim::Op;
use support::{compacted, keyed, rig};

/// A large log: a record of its own in every segment among the hot key's, so that a round would rewrite every segment.
async fn large_log(rig: &support::Rig) {
    rig.create(vec![compacted("compact")], false).await;
    for i in 0..800 {
        let key = if i % 8 == 7 {
            format!("own{i}")
        } else {
            "hot".to_owned()
        };
        let one = keyed(&[(&key, Some(&format!("v{i}")))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
}

fn tmp_files_made(rig: &support::Rig, from: usize) -> usize {
    let trace = rig.fs.trace();
    trace[from..]
        .iter()
        .filter(|(op, path)| *op == Op::Create && path.to_string_lossy().ends_with(".log.tmp"))
        .count()
}

fn tmp_files_left(rig: &support::Rig) -> Vec<String> {
    rig.dirs()
        .iter()
        .flat_map(|d| {
            rig.fs
                .names(std::path::Path::new(&format!("{}/{d}", support::DATA)))
        })
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[tokio::test]
async fn a_stop_while_a_round_is_running_makes_the_round_give_up_before_it_has_made_a_file() {
    let rig = rig();
    large_log(&rig).await;
    let before = rig.fs.trace().len();
    // The round is asked for first and the stop comes while its plan is made: the round must see the stop when it begins.
    let ((), stopped) = tokio::join!(rig.broker.check_cleaner(), rig.broker.shutdown());
    stopped.unwrap();
    assert_eq!(
        tmp_files_made(&rig, before),
        0,
        "the round saw the stop and made no file of its own"
    );
    assert_eq!(tmp_files_left(&rig), Vec::<String>::new());
    // Nothing was cleaned: the log is what it was.
    let again = support::open_on(rig.fs.process_restart());
    assert_eq!(again.list_offset(7, "c", 0, -1).await.offset, 800);
    assert_eq!(
        again.fetch_now(&[("c", 0, 0, 1 << 20)]).await[0].error_code,
        0
    );
}

#[tokio::test]
async fn the_same_round_without_a_stop_makes_files_and_puts_them_in_place() {
    let rig = rig();
    large_log(&rig).await;
    let before = rig.fs.trace().len();
    rig.broker.check_cleaner().await;
    assert!(
        tmp_files_made(&rig, before) > 32,
        "a round of a large log makes a file per segment"
    );
    assert_eq!(tmp_files_left(&rig), Vec::<String>::new());
}
