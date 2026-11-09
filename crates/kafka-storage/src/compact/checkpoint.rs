//! Where cleaning last got to in a partition: the first offset not yet cleaned. A small file beside the segments, replaced
//! atomically and durably; a missing or unreadable one means nothing has been cleaned (the whole log is dirty, which only
//! costs a pass that finds little to remove).

use std::io;
use std::path::{Path, PathBuf};

use kafka_wire::ids::Offset;

use crate::fs::{Fs, read_file, write_durably};

const FILE: &str = "cleaner-checkpoint";

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub(crate) fn read<F: Fs>(fs: &F, dir: &Path) -> Offset {
    let text = read_file(fs, &path(dir)).ok().flatten().unwrap_or_default();
    std::str::from_utf8(&text)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .and_then(Offset::new)
        .unwrap_or(Offset::ZERO)
}

pub(crate) fn write<F: Fs>(fs: &F, dir: &Path, first_dirty: Offset) -> io::Result<()> {
    write_durably(fs, &path(dir), format!("{first_dirty}\n").as_bytes())
}
