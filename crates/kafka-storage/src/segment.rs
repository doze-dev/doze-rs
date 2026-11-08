//! A segment: one log file and its sparse index. Only the active segment (the one with the highest base
//! offset) takes appends; a sealed one is immutable. The two are different types, so appending to a sealed
//! segment is not a mistake the compiler lets through.

mod active;
mod sealed;
mod view;

use std::path::{Path, PathBuf};

pub(crate) use active::ActiveSegment;
pub(crate) use sealed::SealedSegment;
pub(crate) use view::SegmentView;

use kafka_wire::ids::Offset;

/// Every segment's files are named for its base offset: 20 digits, zero padded.
fn stem(base: Offset) -> String {
    format!("{:020}", base.get())
}

pub(crate) fn log_path(dir: &Path, base: Offset) -> PathBuf {
    dir.join(format!("{}.log", stem(base)))
}

pub(crate) fn index_path(dir: &Path, base: Offset) -> PathBuf {
    dir.join(format!("{}.index", stem(base)))
}

/// The base offset a file name stands for, if it is exactly what [`log_path`] and [`index_path`] write: twenty
/// digits and the suffix. Anything else (`5.log`, `+5.log`, `007.log`) is not ours and is left alone: parsing
/// it loosely would name a file that the canonical path does not reach, and the partition would not open.
fn base_of(name: &str, suffix: &str) -> Option<Offset> {
    let stem = name.strip_suffix(suffix)?;
    if stem.len() != 20 || !stem.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok().and_then(Offset::new)
}

/// The base offset a `.log` file name stands for, if it is one of ours.
pub(crate) fn base_of_log(name: &str) -> Option<Offset> {
    base_of(name, ".log")
}

/// The base offset a `.index` file name stands for, if it is one of ours.
pub(crate) fn base_of_index(name: &str) -> Option<Offset> {
    base_of(name, ".index")
}

/// Deletes a segment's files; ones that are not there are not an error.
pub(crate) fn remove_files<F: crate::fs::Fs>(
    fs: &F,
    dir: &Path,
    base: Offset,
) -> std::io::Result<()> {
    fs.remove_file(&log_path(dir, base))?;
    fs.remove_file(&index_path(dir, base))?;
    Ok(())
}

#[cfg(test)]
mod tests;
