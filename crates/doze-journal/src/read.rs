//! Reading the files: the live one and the one rotation set aside, as Go's `journal.Read` does.

use crate::event::{Event, parse_line};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The longest line Go's reader takes (its scanner's buffer). A line this long or longer fails the whole read, as in
/// Go, where no daemon writes one: an event is a few hundred bytes.
pub const MAX_LINE: usize = 1 << 20;

/// The file rotation moved the journal at `path` to: `<path>.1`. One is kept, so the journal is at most two files.
pub fn rotated(path: &Path) -> PathBuf {
    let mut s = OsString::from(path.as_os_str());
    s.push(".1");
    PathBuf::from(s)
}

/// Every event in the journal at `path`, oldest first: the rotated file, then the live one. A missing file is empty;
/// a line that does not decode (a write cut short by a crash) is skipped.
pub fn read(path: &Path) -> io::Result<Vec<Event>> {
    let mut out = read_file(&rotated(path))?;
    out.extend(read_file(path)?);
    Ok(out)
}

/// Every event with a sequence number above `seq`, oldest first. `since(path, 0)` is the whole journal.
pub fn since(path: &Path, seq: u64) -> io::Result<Vec<Event>> {
    let mut all = read(path)?;
    all.retain(|e| e.seq > seq);
    Ok(all)
}

/// The last `n` events, oldest first.
pub fn tail(path: &Path, n: usize) -> io::Result<Vec<Event>> {
    let mut all = read(path)?;
    let cut = all.len().saturating_sub(n);
    all.drain(..cut);
    Ok(all)
}

/// The events in one file.
pub fn read_file(path: &Path) -> io::Result<Vec<Event>> {
    match std::fs::read(path) {
        Ok(bytes) => parse(&bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// The events in one file's bytes, split into lines as Go's `bufio.Scanner` splits them: on `\n`, a `\r` before it
/// dropped, a last line without one kept.
pub fn parse(bytes: &[u8]) -> io::Result<Vec<Event>> {
    let mut out = Vec::new();
    for line in lines(bytes) {
        if line.len() >= MAX_LINE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal line too long",
            ));
        }
        out.extend(parse_line(line.strip_suffix(b"\r").unwrap_or(line)));
    }
    Ok(out)
}

fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let none = bytes.is_empty();
    body.split(|&b| b == b'\n').filter(move |_| !none)
}
