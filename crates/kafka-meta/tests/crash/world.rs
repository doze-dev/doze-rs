//! A random workload against the store, and what it has been told.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use kafka_meta::{MetaError, MetaStore, Topic};
use kafka_storage::fs::Fs;
use kafka_storage::sim::{Cut, DataCut, NameCut, SimFs};
use kafka_wire::ids::{PartitionCount, TopicName};

mod lives;
mod offsets;
mod txns;

pub(crate) const STEPS: usize = 40;
const GROUPS: [&str; 3] = ["g", "g2", "other"];
const NAMES: [&str; 6] = ["a.b", "a_b", "A.b", "orders", "Orders", "x-1"];

/// SplitMix64: a failing run replays from its seed.
pub(crate) struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) % n
    }
}
pub(crate) fn fresh_disk() -> SimFs {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/data")).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    fs
}

pub(crate) fn open(fs: &SimFs) -> Result<MetaStore, MetaError> {
    MetaStore::open(fs, Path::new("/data"))
}

/// What the workload has been told, and what it has attempted without being told.
#[derive(Default)]
pub(crate) struct Model {
    /// Created and acknowledged, and not acknowledged as deleted since.
    pub(crate) acked: HashMap<TopicName, Arc<Topic>>,
    /// Names whose creation failed part-way: they may or may not be there.
    pub(crate) maybe_created: HashSet<TopicName>,
    /// Names in `acked` whose deletion failed part-way: they may or may not still be there.
    pub(crate) maybe_deleted: HashSet<TopicName>,
    /// The largest producer id handed out.
    pub(crate) last_id: Option<i64>,
    /// The committed offsets.
    pub(crate) offsets: offsets::Offsets,
    /// The group records.
    pub(crate) lives: lives::Lives,
    /// The transaction records.
    pub(crate) txns: txns::Txns,
}

impl Model {
    pub(crate) fn check(&self, store: &MetaStore) -> Result<(), String> {
        for (name, topic) in &self.acked {
            match store.topic(name) {
                Some(found) if found == *topic => {}
                Some(found) => return Err(format!("{name} came back as {found:?}, was {topic:?}")),
                None if self.maybe_deleted.contains(name) => {}
                None => return Err(format!("{name} was acknowledged and is gone")),
            }
        }
        for topic in store.topics() {
            let known =
                self.acked.contains_key(&topic.name) || self.maybe_created.contains(&topic.name);
            if !known {
                return Err(format!(
                    "{} is there and was never created, or was deleted",
                    topic.name
                ));
            }
            if store.topic_by_id(topic.id).as_ref() != Some(&topic) {
                return Err(format!("{} is not found by its id", topic.name));
            }
        }
        self.check_offsets(store)?;
        self.check_lives(store)?;
        self.check_txns(store)
    }
}

pub(crate) struct World {
    pub(crate) fs: SimFs,
    pub(crate) store: Option<MetaStore>,
    pub(crate) model: Model,
    pub(crate) rng: Rng,
    pub(crate) seed: u64,
    pub(crate) trail: Vec<&'static str>,
}

impl World {
    pub(crate) fn new(seed: u64, fs: SimFs) -> Self {
        let store = open(&fs).ok();
        Self {
            fs,
            store,
            model: Model::default(),
            rng: Rng(seed),
            seed,
            trail: Vec::new(),
        }
    }

    /// Runs `steps` more; stops at the first write that fails (the process cannot go on, as the broker cannot).
    pub(crate) fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            if self.store.is_none() {
                return;
            }
            let what = match self.rng.below(21) {
                0..=3 => self.create(),
                4..=5 => self.delete(),
                6..=8 => self.producer_id(),
                9..=11 => self.commit(),
                12..=13 => self.life(),
                14 => self.delete_offsets(),
                15 => self.delete_group_offsets(),
                16 => self.forget_life(),
                18..=19 => self.txn(),
                20 => self.forget_txn(),
                17 if self.rng.below(2) == 0 => self.shutdown(),
                _ => self.restart(),
            };
            self.trail.push(what);
            if let Some(Err(e)) = self.store.as_ref().map(|s| self.model.check(s)) {
                panic!(
                    "seed {}: the live store is wrong after {:?}: {e}",
                    self.seed, self.trail
                );
            }
        }
    }

    fn create(&mut self) -> &'static str {
        let name = self.pick();
        let parts = PartitionCount::new(1 + self.rng.below(4) as i32).unwrap();
        let config: BTreeMap<_, _> = (0..self.rng.below(3))
            .map(|i| (format!("k{i}"), format!("v{}", self.rng.below(100))))
            .collect();
        let Some(store) = &self.store else {
            return "create";
        };
        match store.create_topic(&name, parts, config, &HashSet::new()) {
            Ok(topic) => {
                self.model.maybe_deleted.remove(&name);
                self.model.acked.insert(name, topic);
            }
            Err(MetaError::Collides(_) | MetaError::TopicExists(_)) => {}
            Err(_) => {
                self.model.maybe_created.insert(name);
                self.store = None;
            }
        }
        "create"
    }

    fn delete(&mut self) -> &'static str {
        let name = self.pick();
        let Some(store) = &self.store else {
            return "delete";
        };
        match store.delete_topic(&name) {
            Ok(_) => {
                self.model.acked.remove(&name);
                self.model.maybe_deleted.remove(&name);
                self.model.offsets.acked.retain(|k, _| k.1 != name);
                self.model.offsets.maybe.retain(|k, _| k.1 != name);
            }
            Err(_) => {
                self.model.maybe_deleted.insert(name);
                self.store = None;
            }
        }
        "delete"
    }

    fn producer_id(&mut self) -> &'static str {
        if let Some(Ok(id)) = self.store.as_ref().map(MetaStore::next_producer_id) {
            assert!(
                self.model.last_id.is_none_or(|last| id > last),
                "seed {}: an id repeats",
                self.seed
            );
            self.model.last_id = Some(id);
        } else {
            self.store = None;
        }
        "producer_id"
    }

    /// The process is killed (`kill -9`) and started again on the same page cache. A kill runs no destructor,
    /// so the store is forgotten rather than dropped (dropping closes the database, which flushes it).
    fn restart(&mut self) -> &'static str {
        if let Some(store) = self.store.take() {
            std::mem::forget(store);
        }
        self.fs = self.fs.process_restart();
        self.store = open(&self.fs).ok();
        "restart"
    }

    /// The process stops cleanly. If the final flush fails the next process does not get the same page cache:
    /// after a failed flush the kernel has marked the pages clean and a later flush of them says all is well,
    /// so only the machine starting again makes the file trustworthy.
    fn shutdown(&mut self) -> &'static str {
        let Some(store) = self.store.take() else {
            return "shutdown";
        };
        self.fs = match store.close() {
            Ok(()) => self.fs.process_restart(),
            Err(_) => {
                let (data, names, seed) =
                    (DataCut::Pages, NameCut::Random, self.rng.below(u64::MAX));
                self.fs.power_cut(Cut { data, names, seed })
            }
        };
        self.store = open(&self.fs).ok();
        "shutdown"
    }

    fn pick(&mut self) -> TopicName {
        TopicName::parse(NAMES[self.rng.below(NAMES.len() as u64) as usize]).unwrap()
    }
}
