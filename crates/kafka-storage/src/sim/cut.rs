//! Power loss: what the disk holds when the machine stops without warning.
//!
//! A flush makes a file's contents (and, for a directory, its entries) durable. Everything since is only
//! in memory, and a cut keeps some part of it: none, all, a prefix, bytes that read back as zeros, or an
//! arbitrary set of pages. Names behave the same way, apart from the contents of the files they point to.

use std::collections::BTreeSet;

use super::state::{Inode, Namespace, Pending, State};

/// A page: the unit in which dirty data reaches the disk, in any order.
const PAGE: usize = 4096;

/// What of the unflushed contents of a file survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataCut {
    /// Nothing: the file is as of its last flush.
    Drop,
    /// Everything: the machine stopped just after the write reached the disk.
    Keep,
    /// The unflushed writes are there as zeros of the right length (the file was extended, the data was not).
    Zero,
    /// The first part of the unflushed writes, the last of them possibly cut short.
    Prefix,
    /// Each dirty page independently kept or dropped.
    Pages,
}

/// What of the unflushed directory changes survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameCut {
    /// None: a created name is gone and a removed one is back.
    Durable,
    /// All of them.
    Volatile,
    /// Each change independently, in order.
    Random,
}

/// A way for the power to go.
#[derive(Debug, Clone, Copy)]
pub struct Cut {
    /// What of the file contents survives.
    pub data: DataCut,
    /// What of the names survives.
    pub names: NameCut,
    /// Seeds the choices a policy leaves open.
    pub seed: u64,
}

/// A small deterministic random generator (SplitMix64): the choices must replay from a seed.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    fn chance(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

impl State {
    /// The disk after the power goes: a new state with nothing in memory.
    pub(super) fn cut(&self, cut: Cut) -> State {
        let mut rng = Rng::new(cut.seed);
        let mut names = self.durable.clone();
        for op in &self.ns_log {
            let keep = match cut.names {
                NameCut::Durable => false,
                NameCut::Volatile => true,
                NameCut::Random => rng.chance(),
            };
            if keep {
                op.apply(&mut names);
            }
        }
        let mut out = State::default();
        let mut after = Namespace {
            files: std::collections::BTreeMap::new(),
            dirs: names.dirs,
        };
        for (path, id) in names.files {
            let Some(inode) = self.inodes.get(id) else {
                continue;
            };
            let bytes = survivors(inode, cut.data, &mut rng);
            out.inodes.push(Inode {
                data: bytes.clone(),
                durable: bytes,
                pending: Vec::new(),
                mtime_ms: inode.mtime_ms,
                frozen: false,
            });
            after.files.insert(path, out.inodes.len() - 1);
        }
        out.volatile = after.clone();
        out.durable = after;
        out
    }
}

fn survivors(inode: &Inode, policy: DataCut, rng: &mut Rng) -> Vec<u8> {
    match policy {
        DataCut::Drop => inode.durable.clone(),
        DataCut::Keep => inode.data.clone(),
        DataCut::Zero => replay(
            &inode.durable,
            &inode.pending,
            inode.pending.len(),
            true,
            None,
        ),
        DataCut::Prefix => {
            let whole = rng.below(inode.pending.len() + 1);
            let torn = rng.below(PAGE * 2);
            replay(&inode.durable, &inode.pending, whole, false, Some(torn))
        }
        DataCut::Pages => pages(inode, rng),
    }
}

/// `base` with the first `whole` pending changes applied (writes as zeros if `zero`), then, if a `torn`
/// length is given, the next change applied only as far as that many bytes.
fn replay(
    base: &[u8],
    pending: &[Pending],
    whole: usize,
    zero: bool,
    torn: Option<usize>,
) -> Vec<u8> {
    let mut out = base.to_vec();
    for (i, change) in pending.iter().enumerate() {
        let limit = match (i.cmp(&whole), torn) {
            (std::cmp::Ordering::Less, _) => usize::MAX,
            (std::cmp::Ordering::Equal, Some(t)) => t,
            _ => break,
        };
        match change {
            Pending::SetLen(n) => out.resize(*n as usize, 0),
            Pending::Write { pos, data } => {
                let keep = data.len().min(limit);
                let (start, end) = (*pos as usize, *pos as usize + keep);
                if out.len() < end {
                    out.resize(end, 0);
                }
                if let (Some(dst), false) = (out.get_mut(start..end), zero) {
                    dst.copy_from_slice(data.get(..keep).unwrap_or_default());
                }
            }
        }
    }
    out
}

/// Each dirty page independently kept (as in memory) or dropped (as on disk), and a length that is either.
fn pages(inode: &Inode, rng: &mut Rng) -> Vec<u8> {
    let mut dirty = BTreeSet::new();
    for change in &inode.pending {
        match change {
            Pending::Write { pos, data } => {
                let (first, last) = (*pos as usize / PAGE, (*pos as usize + data.len()) / PAGE);
                dirty.extend(first..=last);
            }
            Pending::SetLen(_) => {}
        }
    }
    let len = if rng.chance() {
        inode.data.len()
    } else {
        inode.durable.len()
    };
    let mut out = inode.durable.clone();
    out.resize(inode.data.len().max(inode.durable.len()), 0);
    for page in dirty {
        if !rng.chance() {
            continue;
        }
        let (start, end) = (page * PAGE, ((page + 1) * PAGE).min(out.len()));
        for at in start..end {
            let byte = inode.data.get(at).copied().unwrap_or(0);
            if let Some(slot) = out.get_mut(at) {
                *slot = byte;
            }
        }
    }
    out.resize(len, 0);
    out
}
