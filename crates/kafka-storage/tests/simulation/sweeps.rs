use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, Failure, NameCut, SimFs};
use kafka_storage::{PartitionLog, Trust};
use kafka_wire::batch::TimestampType;

use crate::model::verify;
use crate::world::{CONFIG, NOW, World};

const STEPS: usize = 45;

/// How many seeds each sweep covers: `SIM_SEEDS=500 cargo test` for a longer run.
fn seeds() -> std::ops::RangeInclusive<u64> {
    let n = std::env::var("SIM_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    1..=n
}

fn fresh_disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(std::path::Path::new("/data")).unwrap();
    fs.sync_dir(std::path::Path::new("/")).unwrap();
    fs
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

/// Cuts the power on `world`'s disk in every way, reopens, and checks each against what it was told. Then
/// the reopened log must take an append and serve it.
fn check_every_cut(world: &World, what: &str, strict: bool) {
    for (i, cut) in cuts(world.seed.wrapping_mul(7919)).into_iter().enumerate() {
        let after = world.fs.power_cut(cut);
        let detail = describe(&after, &world.dir);
        let files: Vec<_> = after
            .names(&world.dir)
            .into_iter()
            .map(|n| {
                (
                    after.contents(&world.dir.join(&n)).map_or(0, |c| c.len()),
                    n,
                )
            })
            .collect();
        let mut log = PartitionLog::open(after, world.dir.clone(), CONFIG, Trust::Unvouched)
            .unwrap_or_else(|e| panic!("{what}, cut {i} {cut:?}: the log does not open: {e}"));
        verify(&log, &world.model, strict).unwrap_or_else(|e| {
            let (start, end) = (log.start_offset(), log.end_offset());
            panic!("{what}, cut {i} {cut:?}: {e}\n  files before open: {files:?}{detail}\n  log {start}..{end}, acknowledged to {}, recorded start {}", world.model.synced_end, world.model.durable_start)
        });
        let end = log.end_offset();
        let next = crate::model::Rec {
            offset: end.get(),
            count: 1,
            body: vec![9],
            ts: NOW.get(),
        };
        let appended = log
            .append(&[next.produced()], NOW, TimestampType::Create)
            .unwrap_or_else(|e| panic!("{what}: append after reopen: {e}"));
        assert_eq!(
            appended.first_offset, end,
            "{what}, cut {i}: appends continue where the log ends"
        );
        let got = log.reader().read(end, usize::MAX, None).unwrap();
        assert_eq!(&got, &next.stamped(end.get()), "{what}, cut {i}");
    }
}

/// How many file operations a seed's workload makes.
fn operations(seed: u64) -> u64 {
    let fs = fresh_disk();
    let mut world = World::new(seed, fs.clone());
    world.run(STEPS);
    fs.ops()
}

#[test]
fn the_power_cut_after_each_step_of_a_workload_loses_nothing_acknowledged() {
    for seed in seeds() {
        let fs = fresh_disk();
        let mut world = World::new(seed, fs);
        for step in 0..STEPS {
            world.run(1);
            check_every_cut(&world, &format!("seed {seed}, after step {step}"), false);
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
                false,
            );
        }
    }
}

#[test]
fn any_single_failing_file_operation_loses_nothing_acknowledged() {
    for seed in seeds() {
        let failure = if seed % 2 == 0 {
            Failure::Full
        } else {
            Failure::Io
        };
        let total = operations(seed);
        for n in 1..=total {
            let fs = fresh_disk();
            fs.fail_any(n, failure);
            let mut world = World::new(seed, fs);
            world.run(STEPS);
            check_every_cut(
                &world,
                &format!("seed {seed}, operation {n} of {total} fails with {failure:?}"),
                false,
            );
        }
    }
}

#[test]
fn a_failed_flush_is_never_retried_and_nothing_it_covered_is_lost_that_was_acknowledged() {
    // The kernel's worst case in every position: after a failed flush a file's pages are dropped from the
    // account and a later flush of it succeeds. The log must refuse writes rather than ask again.
    for seed in seeds() {
        let total = operations(seed);
        for n in 1..=total {
            let fs = fresh_disk();
            fs.lose_pages_on_failed_fsync(true);
            fs.fail_any(n, Failure::Io);
            let mut world = World::new(seed, fs);
            world.run(STEPS);
            check_every_cut(
                &world,
                &format!("seed {seed}, flush-loss at operation {n} of {total}"),
                false,
            );
        }
    }
}

/// Reproduces one case a sweep reported and prints the file operations around it:
/// `SIM_REPLAY=291:162 cargo test -p kafka-storage --test simulation --release replay -- --ignored --nocapture`
/// (seed, then the operation to fail; add `:halt` to stop the process after it instead).
#[test]
#[ignore = "a debugging aid"]
fn replay() {
    let spec = std::env::var("SIM_REPLAY").unwrap_or_else(|_| "1:1".into());
    let mut parts = spec.split(':');
    let seed: u64 = parts.next().unwrap().parse().unwrap();
    let n: u64 = parts.next().unwrap().parse().unwrap();
    let flag = parts.next();
    let halt = flag == Some("halt");
    let fs = fresh_disk();
    fs.lose_pages_on_failed_fsync(flag == Some("lose"));
    if halt {
        fs.halt_after(n);
    } else {
        fs.fail_any(
            n,
            if flag == Some("full") {
                Failure::Full
            } else {
                Failure::Io
            },
        );
    }
    let mut world = World::new(seed, fs.clone());
    world.run(STEPS);
    let mut generations = world.earlier.clone();
    generations.push(world.fs.trace());
    for (g, trace) in generations.iter().enumerate() {
        println!("-- process {g}");
        for (i, (op, path)) in trace.iter().enumerate() {
            let name = path.to_string_lossy();
            if (name.contains("0080")
                || name.contains("0095")
                || matches!(
                    op,
                    kafka_storage::sim::Op::DirSync | kafka_storage::sim::Op::Remove
                ))
                && !matches!(op, kafka_storage::sim::Op::Write)
            {
                println!("{:4} {op:?} {}", i + 1, path.display());
            }
        }
    }
    let trace: Vec<(kafka_storage::sim::Op, std::path::PathBuf)> = Vec::new();
    for (i, (op, path)) in trace.iter().enumerate().filter(|(_, (op, p))| {
        let name = p.to_string_lossy();
        name.contains("0080")
            || name.contains("0095")
            || matches!(
                op,
                kafka_storage::sim::Op::DirSync | kafka_storage::sim::Op::Remove
            )
    }) {
        println!("{:4} {op:?} {}", i + 1, path.display());
    }
    println!(
        "steps: {:?}",
        world.trail.iter().take(60).collect::<Vec<_>>()
    );
    println!(
        "synced_end {}, end {}, durable_start {}",
        world.model.synced_end, world.model.end, world.model.durable_start
    );
    check_every_cut(&world, &spec, false);
}

/// What each segment of a disk holds, for reading a failure: the batches (offset, bytes) and the index.
fn describe(fs: &SimFs, dir: &std::path::Path) -> String {
    let mut out = String::new();
    for name in fs.names(dir) {
        let Some(bytes) = fs.contents(&dir.join(&name)) else {
            continue;
        };
        if name.ends_with(".log") {
            let mut at = 0usize;
            let mut batches = Vec::new();
            while let Ok(peek) = kafka_wire::batch::Peek::read(bytes.get(at..).unwrap_or_default())
            {
                batches.push((peek.base_offset, at));
                at += peek.total_len as usize;
            }
            out.push_str(&format!(
                "\n  {name}: {} bytes, batches (offset, at) {batches:?}, valid to {at}",
                bytes.len()
            ));
        } else if name.ends_with(".index") {
            let entries: Vec<_> = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|c| {
                    (
                        i64::from_be_bytes(c[..8].try_into().unwrap()),
                        u64::from_be_bytes(c[8..].try_into().unwrap()),
                    )
                })
                .collect();
            out.push_str(&format!("\n  {name}: entries {entries:?}"));
        }
    }
    out
}
