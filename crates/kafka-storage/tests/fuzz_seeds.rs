//! Writes the seed corpus of the recovery fuzzer (fuzz/seeds/fuzz_recovery): valid logs, a torn one, a damaged
//! index, so the fuzzer starts from inputs that get past the first batch. Run by hand when the format changes:
//! `cargo test -p kafka-storage --features sim --test fuzz_seeds -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use kafka_wire::batch::build::BatchBuilder;

/// One fuzz input: a byte that says where the log ends in what follows, then the log, then the index.
fn input(log: &[u8], index: &[u8]) -> Vec<u8> {
    let rest = log.len() + index.len();
    let split = (log.len() * 256).div_ceil(rest.max(1)).min(255);
    let mut out = vec![u8::try_from(split).unwrap()];
    out.extend_from_slice(log);
    out.extend_from_slice(index);
    out
}

fn run(counts: &[u32]) -> Vec<u8> {
    let mut next = 0;
    let mut out = Vec::new();
    for c in counts {
        out.extend_from_slice(&BatchBuilder::new().base_offset(next).records(*c).build());
        next += i64::from(*c);
    }
    out
}

fn entry(offset: i64, position: i64) -> Vec<u8> {
    [offset.to_be_bytes(), position.to_be_bytes()].concat()
}

#[test]
#[ignore = "writes files"]
fn write_the_fuzz_seeds() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/fuzz_recovery");
    std::fs::create_dir_all(&dir).unwrap();
    let log = run(&[2, 1, 3]);
    let first = BatchBuilder::new().records(2).build().len() as i64;
    let good_index = entry(0, 0);
    let torn = log[..log.len() - 20].to_vec();
    let stale_index = [entry(0, 0), entry(2, first), entry(9, 99_999)].concat();
    let seeds = [
        ("whole", input(&log, &good_index)),
        ("torn-tail", input(&torn, &good_index)),
        ("stale-index", input(&log, &stale_index)),
        ("partial-index-entry", input(&log, &good_index[..9])),
        ("no-index", input(&log, &[])),
        ("empty", input(&[], &[])),
    ];
    for (name, bytes) in seeds {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}
