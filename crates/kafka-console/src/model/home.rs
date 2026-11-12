//! The view-model of Home, in its three states.

use super::{Access, ProbeView};

/// Which Home the data calls for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeState {
    /// Something is running and nothing needs anyone.
    Steady,
    /// There is nothing in the broker yet: no topics.
    Quiet,
    /// A transaction is close to its timeout.
    Attention(Attention),
}

/// The one condition Home draws as needing someone: a transaction near its timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    /// How many transactions are close (the first is described).
    pub count: usize,
    pub transactional_id: String,
    pub producer_id: i64,
    pub producer_epoch: i16,
    /// `orders-0`.
    pub partition: String,
    pub topic: String,
    /// `22:13:33 UTC`.
    pub since: String,
    pub timeout_secs: i64,
    pub elapsed_secs: i64,
    /// What a `read_committed` consumer of the partition is held back by, if it was asked.
    pub held_back: Option<HeldBack>,
}

/// The gap between the last stable offset and the high watermark of a partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldBack {
    pub last_stable: i64,
    pub high_watermark: i64,
}

/// What the last stop was, for the instance card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopView {
    None,
    Clean,
    Unclean,
}

/// The "This instance" card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceView {
    /// `Nov 6 18:02 UTC`.
    pub started: String,
    pub up_secs: i64,
    pub last_stop: StopView,
    pub open_partitions: usize,
    /// 0: no bound.
    pub max_open_partitions: usize,
    pub open_files: Option<usize>,
    pub resident_bytes: Option<u64>,
    pub threads: Option<u32>,
    pub profile: String,
    pub access: Access,
    pub data_dir: String,
}

/// Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeView {
    pub state: HomeState,
    pub listen: String,
    pub loopback: bool,
    pub topics: usize,
    pub partitions: usize,
    pub probe: ProbeView,
    pub instance: InstanceView,
}
