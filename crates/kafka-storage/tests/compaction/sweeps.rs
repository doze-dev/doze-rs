use std::collections::HashMap;

use kafka_storage::Trust;
use std::path::PathBuf;

use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};

use crate::world::{PARAMS, Seen, clean_fully, holds, open, records, written};

#[test]
fn random_workloads_keep_the_latest_of_every_key_and_clean_to_one_record_a_key_below_the_reach() {
    for seed in 1..=40 {
        let (fs, dir, truth) = written(seed, 30 + usize::try_from(seed % 5).unwrap() * 8, 6);
        let disk = fs.process_restart();
        let mut log = open(&disk, &dir, Trust::Vouched);
        holds(&log, &truth, &format!("seed {seed} before"));
        let reach = clean_fully(&mut log);
        holds(&log, &truth, &format!("seed {seed} after"));
        if let Some(reach) = reach {
            let mut seen = std::collections::HashSet::new();
            for (o, k, _) in records(&log) {
                if o < reach.clean_to {
                    assert!(
                        seen.insert(k),
                        "seed {seed}: key {k} twice below {}",
                        reach.clean_to
                    );
                }
            }
        }
        // And it is the same log after a restart.
        let again = open(&disk.process_restart(), &dir, Trust::Vouched);
        assert_eq!(records(&again), records(&log), "seed {seed}");
    }
}

/// The ways the process can end: killed (the disk keeps all it was given), or the power goes with each of the ways the
/// disk may keep or lose what had not been flushed.
pub(crate) fn endings(stop: u64) -> Vec<Option<Cut>> {
    let mut all = vec![None];
    let data = [
        DataCut::Drop,
        DataCut::Keep,
        DataCut::Zero,
        DataCut::Prefix,
        DataCut::Pages,
    ];
    for d in data {
        for names in [NameCut::Durable, NameCut::Volatile] {
            all.push(Some(Cut {
                data: d,
                names,
                seed: stop,
            }));
        }
    }
    // Which of the names changed since the last flush survive is left to chance, so it is tried several ways.
    for d in [DataCut::Drop, DataCut::Prefix] {
        for seed in 0..8 {
            all.push(Some(Cut {
                data: d,
                names: NameCut::Random,
                seed: stop * 8 + seed,
            }));
        }
    }
    all
}

/// A round stopped after `stop` file operations; what the disk then holds.
fn stopped(seed: u64, stop: u64, end: Option<Cut>) -> (SimFs, PathBuf, Vec<Seen>) {
    let (fs, dir, truth) = written(seed, 36, 6);
    let mut log = open(&fs, &dir, Trust::Vouched);
    fs.halt_after(stop);
    let job = log.clean_job(&PARAMS, log.end_offset(), vec![], HashMap::new());
    if let Some(cleaned) = job.ok().flatten().and_then(|job| job.run().ok()) {
        let _ = log.install_cleaned(&cleaned);
    }
    drop(log);
    let disk = end.map_or_else(|| fs.process_restart(), |cut| fs.power_cut(cut));
    (disk, dir, truth)
}

#[test]
fn a_stop_between_any_two_steps_of_a_round_loses_nothing_and_the_round_run_again_ends_where_it_would_have()
 {
    for seed in [3, 29] {
        // How many operations a round takes, and where an uninterrupted one ends.
        let (fs, dir, truth) = written(seed, 36, 6);
        let mut log = open(&fs, &dir, Trust::Vouched);
        let before = fs.ops();
        clean_fully(&mut log);
        let steps = fs.ops() - before;
        let want = records(&log);
        assert!(steps > 20, "{steps}");
        for stop in 0..=steps {
            for end in endings(stop) {
                let what = format!("seed {seed}, stop {stop}/{steps}, {end:?}");
                let (disk, dir, truth_again) = stopped(seed, stop, end);
                assert_eq!(truth, truth_again);
                let log = open(&disk, &dir, Trust::Unvouched);
                holds(&log, &truth, &what);
                drop(log);
                let mut log = open(&disk.process_restart(), &dir, Trust::Unvouched);
                holds(&log, &truth, &format!("{what}, reopened"));
                clean_fully(&mut log);
                holds(&log, &truth, &format!("{what}, cleaned"));
                assert_eq!(
                    records(&log),
                    want,
                    "{what}: cleaned again it is not what an uninterrupted round leaves"
                );
            }
        }
    }
}
