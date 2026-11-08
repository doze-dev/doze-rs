//! The recorded log-start offset: where DeleteRecords put the start, which a segment's name cannot say
//! (segments are named for the offset they begin at, and the new start is usually in the middle of one).
//!
//! It is a small file beside the segments, replaced atomically and durably. The log start is the larger of
//! it and the first segment's base.

use std::io;
use std::path::{Path, PathBuf};

use kafka_wire::ids::Offset;

use crate::fs::{Fs, read_file, write_durably};

const FILE: &str = "log-start";

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The recorded start of the partition in `dir`, or 0 when none was recorded or the file is unreadable (in
/// which case nothing is hidden: the segments are what is on disk).
pub(crate) fn read<F: Fs>(fs: &F, dir: &Path) -> Offset {
    let text = read_file(fs, &path(dir)).ok().flatten().unwrap_or_default();
    std::str::from_utf8(&text)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .and_then(Offset::new)
        .unwrap_or(Offset::ZERO)
}

/// Records `start`: a crash leaves the old value or the new, and a power cut after this returns does not bring
/// the old one back.
pub(crate) fn write<F: Fs>(fs: &F, dir: &Path, start: Offset) -> io::Result<()> {
    write_durably(fs, &path(dir), format!("{start}\n").as_bytes())
}
