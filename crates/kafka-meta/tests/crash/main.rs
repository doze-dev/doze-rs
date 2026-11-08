//! A random workload against the metadata store on the simulated disk, with the power cut and files failing at
//! every point: everything acknowledged must be there afterwards, and nothing that was never attempted.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::collections::{BTreeMap, HashSet};

use kafka_storage::sim::{Cut, DataCut, Failure, NameCut};
use kafka_wire::ids::{PartitionCount, TopicName};

mod world;
use world::{STEPS, World, fresh_disk, open};

fn seeds() -> std::ops::RangeInclusive<u64> {
    let n = std::env::var("SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    1..=n
}

/// Every way the power can go that the model knows.
fn cuts(seed: u64) -> [Cut; 6] {
    let cut = |data, names| Cut { data, names, seed };
    [
        cut(DataCut::Drop, NameCut::Durable),
        cut(DataCut::Zero, NameCut::Durable),
        cut(DataCut::Prefix, NameCut::Random),
        cut(DataCut::Pages, NameCut::Random),
        cut(DataCut::Keep, NameCut::Volatile),
        cut(DataCut::Drop, NameCut::Volatile),
    ]
}

/// Cuts the power on `world`'s disk in every way and reopens: all that was acknowledged must be there, no id
/// may repeat, and the reopened store must take a write.
fn check_every_cut(world: &World, what: &str) {
    for (i, cut) in cuts(world.seed.wrapping_mul(7919)).into_iter().enumerate() {
        let after = world.fs.power_cut(cut);
        let store =
            open(&after).unwrap_or_else(|e| panic!("{what}, cut {i} {cut:?}: does not open: {e}"));
        world
            .model
            .check(&store)
            .unwrap_or_else(|e| panic!("{what}, cut {i} {cut:?}: {e}"));
        let id = store
            .next_producer_id()
            .unwrap_or_else(|e| panic!("{what}, cut {i}: no id: {e}"));
        assert!(
            world.model.last_id.is_none_or(|last| id > last),
            "{what}, cut {i}: id {id} repeats"
        );
        let name = TopicName::parse("after-the-cut").unwrap();
        let parts = PartitionCount::new(1).unwrap();
        store
            .create_topic(&name, parts, BTreeMap::new(), &HashSet::new())
            .unwrap_or_else(|e| panic!("{what}, cut {i}: cannot create: {e}"));
    }
}

/// How many counted file operations a seed's workload makes.
fn operations(seed: u64) -> u64 {
    let fs = fresh_disk();
    World::new(seed, fs.clone()).run(STEPS);
    fs.ops()
}

#[test]
fn the_power_cut_after_each_step_of_a_workload_loses_nothing_acknowledged() {
    for seed in seeds() {
        let mut world = World::new(seed, fresh_disk());
        for step in 0..STEPS {
            world.run(1);
            check_every_cut(&world, &format!("seed {seed}, after step {step}"));
        }
    }
}

#[test]
fn stopping_the_process_after_every_file_operation_loses_nothing_acknowledged() {
    for seed in seeds() {
        let total = operations(seed);
        for n in 1..=total {
            let fs = fresh_disk();
            fs.halt_after(n);
            let mut world = World::new(seed, fs);
            world.run(STEPS);
            check_every_cut(
                &world,
                &format!("seed {seed}, stopped after operation {n} of {total}"),
            );
        }
    }
}

#[test]
fn any_single_failing_file_operation_loses_nothing_acknowledged() {
    for seed in seeds() {
        let total = operations(seed);
        for n in 1..=total {
            let fs = fresh_disk();
            // Alternate the kernel's two ways of failing a flush: reported once, or reported once and then
            // the pages are dropped and a later flush says all is well.
            fs.lose_pages_on_failed_fsync(seed.is_multiple_of(2));
            fs.fail_any(
                n,
                if seed.is_multiple_of(3) {
                    Failure::Full
                } else {
                    Failure::Io
                },
            );
            let mut world = World::new(seed, fs);
            world.run(STEPS);
            check_every_cut(
                &world,
                &format!("seed {seed}, operation {n} of {total} fails"),
            );
        }
    }
}

/// Reproduces one case a sweep reported and prints the steps and the file operations:
/// `SIM_REPLAY=2:20 cargo test -p kafka-meta --test crash --release replay -- --ignored --nocapture`
/// (seed, then the operation to fail; a third part `halt` stops the process after it instead).
#[test]
#[ignore = "a debugging aid"]
fn replay() {
    let spec = std::env::var("SIM_REPLAY").unwrap_or_else(|_| "1:1".into());
    let mut parts = spec.split(':');
    let seed: u64 = parts.next().unwrap().parse().unwrap();
    let n: u64 = parts.next().unwrap().parse().unwrap();
    let fs = fresh_disk();
    if parts.next() == Some("halt") {
        fs.halt_after(n);
    } else {
        fs.lose_pages_on_failed_fsync(seed.is_multiple_of(2));
        fs.fail_any(
            n,
            if seed.is_multiple_of(3) {
                Failure::Full
            } else {
                Failure::Io
            },
        );
    }
    let mut world = World::new(seed, fs.clone());
    for _ in 0..STEPS {
        world.run(1);
        println!(
            "after {:?}: {} operations; acked {:?}; maybe created {:?}; maybe deleted {:?}",
            world.trail.last(),
            fs.ops(),
            world
                .model
                .acked
                .keys()
                .map(TopicName::as_str)
                .collect::<Vec<_>>(),
            world.model.maybe_created,
            world.model.maybe_deleted
        );
    }
    for (i, (op, path)) in fs.trace().iter().enumerate() {
        println!("{:3} {op:?} {}", i + 1, path.display());
    }
    println!("steps: {:?}", world.trail);
    check_every_cut(&world, &spec);
}
