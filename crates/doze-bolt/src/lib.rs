//! Reading a bbolt file, the database the Go doze-kafka and doze-aws kept their metadata in, so the Rust editions can
//! take over a data directory Go left (docs/port/modules/PLAN.md, step 14 and 15: data compatibility).
//!
//! Read-only, whole file in memory, no `unsafe`: the files are small (the metadata, never the data), and a corrupt or
//! hostile file is an error, never a panic or a loop. The layout is bbolt 1.3 to 1.5's (format version 2), little-endian
//! as Go wrote it on every platform doze ships for:
//!
//! - pages of `page_size` bytes; a page is a 16-byte header (id u64, flags u16, count u16, overflow u32) then its
//!   elements, and spans `overflow + 1` pages;
//! - pages 0 and 1 are metas (magic, version, page size, flags, root bucket, freelist, high page, txid, FNV-64a
//!   checksum); the valid one with the higher txid is the database;
//! - a branch element is (pos u32, ksize u32, pgid u64), a leaf element (flags u32, pos u32, ksize u32, vsize u32),
//!   each `pos` counted from the element itself;
//! - a leaf element with flag 1 is a sub-bucket: its value is (root pgid u64, sequence u64), and a root of 0 means the
//!   bucket's own page follows inline in the value.

use std::path::Path;

const HEADER: usize = 16;
const ELEMENT: usize = 16;
const MAGIC: u32 = 0xED0C_DAED;
const VERSION: u32 = 2;
const BRANCH: u16 = 0x01;
const LEAF: u16 = 0x02;
const BUCKET_LEAF: u32 = 0x01;
/// How deep a tree may be (bbolt's are a handful of levels; this only stops a corrupt file looping).
const DEPTH: usize = 64;

/// What one walk of a tree may cost, so a damaged or hostile file (pages that point at each other, values that overlap)
/// costs at most a few times its own size. A real tree visits each page once and copies each key and value once.
#[derive(Debug)]
struct Budget {
    /// Bytes of pages still to visit.
    visit: usize,
    /// Bytes of keys and values still to copy.
    copy: usize,
}

impl Budget {
    fn of(file: usize) -> Budget {
        Budget {
            visit: file.saturating_mul(2),
            copy: file.saturating_mul(4).saturating_add(1 << 20),
        }
    }

    fn spend(have: &mut usize, n: usize) -> Result<(), Error> {
        *have = have
            .checked_sub(n)
            .ok_or_else(|| Error::Corrupt("the tree is larger than the file".into()))?;
        Ok(())
    }
}

/// Why a file cannot be read.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// Not a bbolt file, or a damaged one; what was wrong.
    Corrupt(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Corrupt(why) => write!(f, "not a readable bbolt file: {why}"),
        }
    }
}

impl std::error::Error for Error {}

fn corrupt<T>(why: impl Into<String>) -> Result<T, Error> {
    Err(Error::Corrupt(why.into()))
}

/// What a key holds: a value, or a bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Value(Vec<u8>),
    Bucket,
}

/// A key, what it holds, and a sub-bucket's root beside it.
type Entry = (Vec<u8>, Item, Option<Root>);

/// A key and its value.
pub type Pair = (Vec<u8>, Vec<u8>);

/// Where a bucket's tree is.
#[derive(Debug, Clone)]
enum Root {
    Page(u64),
    Inline(Vec<u8>),
}

/// A bbolt database, read.
#[derive(Debug)]
pub struct Db {
    bytes: Vec<u8>,
    page: usize,
    /// Pages in use: no reference may point at or past it.
    high: u64,
    root: u64,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn fnv64a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A meta page's (page size, root, high page, txid), when it is valid.
fn meta(bytes: &[u8], at: usize) -> Option<(usize, u64, u64, u64)> {
    let m = bytes.get(at + HEADER..at + HEADER + 64)?;
    let ok = u32_at(m, 0)? == MAGIC
        && u32_at(m, 4)? == VERSION
        && u64_at(m, 56)? == fnv64a(m.get(..56)?);
    let page = usize::try_from(u32_at(m, 8)?).ok()?;
    (ok && page.is_power_of_two() && (512..=1 << 20).contains(&page))
        .then(|| Some((page, u64_at(m, 16)?, u64_at(m, 40)?, u64_at(m, 48)?)))
        .flatten()
}

impl Db {
    /// Reads the file at `path`.
    ///
    /// # Errors
    /// The file cannot be read, or is not a bbolt database.
    pub fn open(path: &Path) -> Result<Db, Error> {
        Db::parse(std::fs::read(path).map_err(Error::Io)?)
    }

    /// A database from its bytes.
    ///
    /// # Errors
    /// Not a bbolt database, or neither meta page is valid.
    // fuzz:parser fuzz_bolt
    pub fn parse(bytes: Vec<u8>) -> Result<Db, Error> {
        let first = meta(&bytes, 0);
        // The second meta is one page in; the first gives the page size, or each size bbolt may use is tried.
        let second = first
            .map(|m| vec![m.0])
            .unwrap_or_else(|| (9..=16).map(|s| 1 << s).collect())
            .into_iter()
            .find_map(|size| meta(&bytes, size).filter(|m| m.0 == size));
        let best = match (first, second) {
            (Some(a), Some(b)) => Some(if b.3 > a.3 { b } else { a }),
            (a, b) => a.or(b),
        };
        let Some((page, root, high, _)) = best else {
            return corrupt("no valid meta page");
        };
        Ok(Db {
            bytes,
            page,
            high,
            root,
        })
    }

    /// The page `id` and the pages it overflows into.
    fn page(&self, id: u64) -> Result<&[u8], Error> {
        if id < 2 || id >= self.high {
            return corrupt(format!("page {id} is outside the tree"));
        }
        let start = usize::try_from(id)
            .ok()
            .and_then(|i| i.checked_mul(self.page));
        let head = start.and_then(|s| self.bytes.get(s..s.checked_add(HEADER)?));
        let (Some(start), Some(head)) = (start, head) else {
            return corrupt(format!("page {id} is past the end of the file"));
        };
        let overflow = u32_at(head, 12).and_then(|o| usize::try_from(o).ok());
        let len = overflow.and_then(|o| o.checked_add(1)?.checked_mul(self.page));
        match len.and_then(|l| self.bytes.get(start..start.checked_add(l)?)) {
            Some(p) => Ok(p),
            None => corrupt(format!("page {id} runs past the end of the file")),
        }
    }

    /// Every key of the tree at `root`, in order, with what it holds (a sub-bucket's own root beside it).
    fn read(&self, root: &Root) -> Result<Vec<Entry>, Error> {
        let mut out = Vec::new();
        let mut budget = Budget::of(self.bytes.len());
        match root {
            Root::Inline(page) => self.node(page, 0, &mut budget, &mut out)?,
            Root::Page(id) => self.node(self.page(*id)?, 0, &mut budget, &mut out)?,
        }
        Ok(out)
    }

    fn node(
        &self,
        p: &[u8],
        depth: usize,
        budget: &mut Budget,
        out: &mut Vec<Entry>,
    ) -> Result<(), Error> {
        if depth > DEPTH {
            return corrupt("the tree is deeper than any bbolt makes");
        }
        Budget::spend(&mut budget.visit, p.len().max(HEADER))?;
        let (Some(flags), Some(count)) = (u16_at(p, 8), u16_at(p, 10)) else {
            return corrupt("a page header is cut short");
        };
        for i in 0..usize::from(count) {
            let at = HEADER + i * ELEMENT;
            let field = |off: usize| {
                u32_at(p, at + off)
                    .and_then(|v| usize::try_from(v).ok())
                    .ok_or_else(|| Error::Corrupt("an element is cut short".into()))
            };
            let slice = |from: usize, len: usize| {
                from.checked_add(len)
                    .and_then(|end| p.get(from..end))
                    .ok_or_else(|| Error::Corrupt("a key or value is outside its page".into()))
            };
            match flags {
                BRANCH => {
                    let child = u64_at(p, at + 8)
                        .ok_or_else(|| Error::Corrupt("an element is cut short".into()))?;
                    self.node(self.page(child)?, depth + 1, budget, out)?;
                }
                LEAF => {
                    let (kind, pos, ksize, vsize) = (field(0)?, field(4)?, field(8)?, field(12)?);
                    let key = slice(at.saturating_add(pos), ksize)?.to_vec();
                    let value = slice(at.saturating_add(pos).saturating_add(ksize), vsize)?;
                    Budget::spend(&mut budget.copy, ksize.saturating_add(vsize))?;
                    if kind & BUCKET_LEAF as usize == 0 {
                        out.push((key, Item::Value(value.to_vec()), None));
                        continue;
                    }
                    let Some(sub) = u64_at(value, 0) else {
                        return corrupt("a bucket header is cut short");
                    };
                    let root = if sub == 0 {
                        Root::Inline(value.get(HEADER..).unwrap_or_default().to_vec())
                    } else {
                        Root::Page(sub)
                    };
                    out.push((key, Item::Bucket, Some(root)));
                }
                other => return corrupt(format!("a page of kind {other:#x} inside a tree")),
            }
        }
        Ok(())
    }

    /// The root of the bucket at `path` (bucket names from the top), if it is there.
    fn find(&self, path: &[&[u8]]) -> Result<Option<Root>, Error> {
        let mut root = Root::Page(self.root);
        for name in path {
            let found = self
                .read(&root)?
                .into_iter()
                .find(|(k, _, _)| k.as_slice() == *name)
                .and_then(|(_, _, r)| r);
            match found {
                Some(r) => root = r,
                None => return Ok(None),
            }
        }
        Ok(Some(root))
    }

    /// Every key of the bucket at `path`, in order, with what it holds; empty when there is no such bucket.
    ///
    /// # Errors
    /// The file is damaged.
    pub fn items(&self, path: &[&[u8]]) -> Result<Vec<(Vec<u8>, Item)>, Error> {
        let Some(root) = self.find(path)? else {
            return Ok(Vec::new());
        };
        Ok(self
            .read(&root)?
            .into_iter()
            .map(|(k, i, _)| (k, i))
            .collect())
    }

    /// The key/value pairs of the bucket at `path` (its sub-buckets left out); empty when there is no such bucket.
    ///
    /// # Errors
    /// The file is damaged.
    pub fn values(&self, path: &[&[u8]]) -> Result<Vec<Pair>, Error> {
        Ok(self
            .items(path)?
            .into_iter()
            .filter_map(|(k, i)| match i {
                Item::Value(v) => Some((k, v)),
                Item::Bucket => None,
            })
            .collect())
    }

    /// The names of the sub-buckets of the bucket at `path` (`&[]`: the top-level buckets).
    ///
    /// # Errors
    /// The file is damaged.
    pub fn buckets(&self, path: &[&[u8]]) -> Result<Vec<Vec<u8>>, Error> {
        Ok(self
            .items(path)?
            .into_iter()
            .filter_map(|(k, i)| (i == Item::Bucket).then_some(k))
            .collect())
    }

    /// Whether any bucket of the database holds a value (a fresh file Go made and never wrote to holds none, but for
    /// the schema version every Go store records in `_schema`).
    ///
    /// # Errors
    /// The file is damaged.
    pub fn holds_data(&self) -> Result<bool, Error> {
        for name in self.buckets(&[])? {
            if name != b"_schema" && !self.items(&[&name])?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests;
