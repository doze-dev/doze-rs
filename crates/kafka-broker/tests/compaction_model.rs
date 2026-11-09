//! A model run: random workloads of plain, idempotent and transactional writes, cleaning rounds, restarts and DeleteRecords
//! go through the broker's handlers, and whatever the order of those, a consumer still finds the latest committed record of
//! every key, in order, from every offset and after a restart.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use std::collections::HashMap;

use support::{
    Rig, Txn, clean_rounds, compacted, consume, delete_records, from, keyed, open_on, rig,
    txn_produce,
};

/// How many random workloads are run (a fixed seed each, so that a failure is repeatable).
const SEEDS: u64 = 200;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// What the model expects: the offset of each key's latest committed record, and the log start.
#[derive(Default)]
struct Model {
    latest: HashMap<String, i64>,
    start: i64,
}

struct World {
    rig: Rig,
    rng: Rng,
    model: Model,
    idempotent: (i64, i16, i32),
    txns: [(&'static str, Txn, i32); 2],
    history: Vec<String>,
}

impl World {
    async fn new(seed: u64) -> Self {
        let rig = rig();
        rig.create(vec![compacted("compact,delete")], false).await;
        let p = rig.init_producer_id(None).await;
        let ta = rig.init_txn("ta", 60_000).await.unwrap();
        let tb = rig.init_txn("tb", 60_000).await.unwrap();
        World {
            rig,
            rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
            model: Model::default(),
            idempotent: (p.producer_id.0, p.producer_epoch, 0),
            txns: [("ta", ta, 0), ("tb", tb, 0)],
            history: Vec::new(),
        }
    }

    async fn plain(&mut self, key: &str) {
        let r = self
            .rig
            .produce(-1, "c", 0, keyed(&[(key, Some("v"))]))
            .await;
        assert_eq!(r.error_code, 0);
        self.model.latest.insert(key.to_owned(), r.base_offset);
        self.history.push(format!("plain {key} @{}", r.base_offset));
    }

    async fn idempotent(&mut self, key: &str) {
        let (id, epoch, seq) = self.idempotent;
        let r = self
            .rig
            .produce(-1, "c", 0, from(id, epoch, seq, false, &[(key, "v")]))
            .await;
        assert_eq!(
            r.error_code, 0,
            "idempotent seq {seq} after {:?}",
            self.history
        );
        self.idempotent.2 += 1;
        self.model.latest.insert(key.to_owned(), r.base_offset);
        self.history.push(format!("idem {key} @{}", r.base_offset));
    }

    async fn transaction(&mut self, key: &str) {
        let i = usize::from(self.rng.below(2) == 0);
        let (name, t, seq) = self.txns[i];
        assert_eq!(self.rig.add_partitions(name, t, "c", &[0]).await, [0]);
        let batch = from(t.producer, t.epoch, seq, true, &[(key, "v")]);
        let (e, base) = txn_produce(&self.rig, name, batch).await;
        assert_eq!(e, 0, "{name} seq {seq} after {:?}", self.history);
        self.txns[i].2 += 1;
        let commit = self.rng.below(3) != 0;
        assert_eq!(self.rig.end_txn(name, t, commit).await, 0);
        if commit {
            self.model.latest.insert(key.to_owned(), base);
        }
        self.history
            .push(format!("txn {name} {key} @{base} commit={commit}"));
    }

    async fn clean(&mut self) {
        clean_rounds(&self.rig, 1 + self.rng.below(2) as usize).await;
        self.history.push("clean".into());
    }

    async fn restart_or_delete(&mut self) {
        if self.rng.below(2) == 0 {
            self.rig = open_on(self.rig.fs.process_restart());
            self.history.push("restart".into());
            return;
        }
        let end = self.rig.list_offset(7, "c", 0, -1).await.offset;
        let start = self.model.start;
        if end > start + 2 {
            let target = start + 1 + self.rng.below((end - start) as u64 - 1) as i64;
            assert_eq!(delete_records(&self.rig, target).await, (0, target));
            self.model.start = target;
            self.history.push(format!("delete_records {target}"));
        }
    }

    async fn step(&mut self) {
        let key = format!("k{}", self.rng.below(4));
        match self.rng.below(10) {
            0..=2 => self.plain(&key).await,
            3 | 4 => self.idempotent(&key).await,
            5..=7 => self.transaction(&key).await,
            8 => self.clean().await,
            _ => self.restart_or_delete().await,
        }
    }

    /// What a consumer must find, in both isolation levels, from the start and from every offset, and after a restart.
    async fn check(&self, seed: u64) {
        let (rig, start) = (&self.rig, self.model.start);
        let end = rig.list_offset(7, "c", 0, -1).await.offset;
        assert_eq!(
            rig.list_offset(7, "c", 0, -2).await.offset,
            start,
            "seed {seed}: start"
        );
        for committed in [false, true] {
            let seen = consume(rig, start, committed).await;
            for (k, off) in self.model.latest.iter().filter(|(_, o)| **o >= start) {
                assert!(
                    seen.iter().any(|(o, kk)| o == off && kk == k),
                    "seed {seed} committed={committed}: latest committed {k}@{off} missing; end {end}, start {start}; seen {seen:?}; history {:?}",
                    self.history
                );
            }
            assert!(
                seen.windows(2).all(|w| w[0].0 < w[1].0),
                "seed {seed}: order"
            );
            assert!(
                seen.iter().all(|(o, _)| *o >= start && *o < end),
                "seed {seed}: range"
            );
        }
        for o in start..end {
            for committed in [false, true] {
                let got = consume(rig, o, committed).await;
                assert!(got.iter().all(|(oo, _)| *oo >= o), "seed {seed}: from {o}");
            }
        }
        let again = open_on(rig.fs.process_restart());
        let (a, b) = (
            consume(&again, start, false).await,
            consume(rig, start, false).await,
        );
        assert_eq!(a, b, "seed {seed}: after a restart");
    }
}

#[tokio::test]
async fn random_workloads_through_the_broker_keep_the_latest_committed_record_of_every_key() {
    for seed in 1..=SEEDS {
        eprintln!("seed {seed}");
        let mut world = World::new(seed).await;
        for _ in 0..60 + world.rng.below(60) {
            world.step().await;
        }
        clean_rounds(&world.rig, 3).await;
        world.check(seed).await;
    }
}
