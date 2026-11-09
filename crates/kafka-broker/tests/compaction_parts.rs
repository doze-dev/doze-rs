//! A round of cleaning is put in place a few segments at a time, so that the partition's writer takes the appends that have
//! waited between two parts and is not held for the whole round.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::cell::Cell;
use std::path::PathBuf;

use kafka_storage::sim::Op;
use support::{compacted, keyed, rig};

/// Where in a trace a segment's replacement (`<base>.log.tmp`) is renamed into place.
fn replaced_at(trace: &[(Op, PathBuf)]) -> Vec<usize> {
    trace
        .iter()
        .enumerate()
        .filter(|(_, (op, path))| *op == Op::Rename && path.to_string_lossy().ends_with(".log.tmp"))
        .map(|(i, _)| i)
        .collect()
}

/// Where in a trace bytes are written to a segment's log, which are appends (the cleaner writes its own files as `.tmp`).
fn appended_at(trace: &[(Op, PathBuf)]) -> Vec<usize> {
    trace
        .iter()
        .enumerate()
        .filter(|(_, (op, path))| *op == Op::Write && path.extension().is_some_and(|e| e == "log"))
        .map(|(i, _)| i)
        .collect()
}

#[tokio::test]
async fn an_append_that_arrives_while_a_round_is_being_put_in_place_is_taken_before_the_round_is_all_in_place()
 {
    let rig = rig();
    rig.create(vec![compacted("compact")], false).await;
    // Segments of a kilobyte hold about a dozen of these, and each keeps a record of its own among the hot key's, so that
    // each is rewritten (not just removed): far more replacements than the broker puts in place at once.
    for i in 0..800 {
        let key = if i % 8 == 7 {
            format!("own{i}")
        } else {
            "hot".to_owned()
        };
        let one = keyed(&[(&key, Some(&format!("v{i}")))]);
        assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
    }
    let before = rig.fs.trace().len();
    let finished = Cell::new(false);
    let appender = async {
        // An append at a time, as fast as they are answered, until the round is done.
        while !finished.get() {
            let one = keyed(&[("k0", Some("while cleaning"))]);
            assert_eq!(rig.produce(-1, "c", 0, one).await.error_code, 0);
        }
    };
    let round = async {
        rig.broker.check_cleaner().await;
        finished.set(true);
    };
    tokio::join!(round, appender);
    let trace = rig.fs.trace()[before..].to_vec();
    let (in_place, appends) = (replaced_at(&trace), appended_at(&trace));
    let (first, last) = (in_place[0], *in_place.last().unwrap());
    assert!(
        in_place.len() > 32,
        "a round of more than two parts: {}",
        in_place.len()
    );
    let between = appends.iter().filter(|a| **a > first && **a < last).count();
    assert!(
        between > 0,
        "no append was written between the first segment put in place (operation {first}) and the last ({last}) of {} replacements",
        in_place.len()
    );
}
