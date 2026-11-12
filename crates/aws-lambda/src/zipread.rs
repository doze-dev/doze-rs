//! A zip archive unpacked to a directory, the way Lambda unpacks a deployment package: stored and deflated entries, the central
//! directory trusted over the local headers, every entry's size and CRC checked, unix permission bits kept (a `bootstrap` must stay
//! executable), and nothing written outside the destination. Bounded: AWS allows 250 MB unzipped, so an archive that inflates past
//! that, or holds more than 100,000 entries, is refused before it can fill the disk. Zip64 archives are refused (the package limit
//! is 50 MB zipped, far under 4 GB).

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use flate2::read::DeflateDecoder;

/// The most unpacked bytes (Lambda's limit: 262,144,000).
pub(crate) const MAX_UNPACKED: u64 = 262_144_000;
const MAX_ENTRIES: usize = 100_000;

/// Why an archive was not unpacked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ZipError {
    /// Not a zip, or damaged.
    Malformed,
    /// Larger unpacked than Lambda allows, or with too many entries.
    TooBig,
    /// The disk refused.
    Io,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// One entry of the central directory.
struct Entry<'a> {
    name: &'a str,
    method: u16,
    crc: u32,
    packed: usize,
    size: u64,
    mode: Option<u32>,
    local: usize,
}

/// The end-of-central-directory record: (entry count, directory offset).
fn end_record(zip: &[u8]) -> Option<(usize, usize)> {
    let floor = zip.len().saturating_sub(65_557);
    let at = (floor..zip.len().checked_sub(21)?)
        .rev()
        .find(|i| u32_at(zip, *i) == Some(0x0605_4b50))?;
    let count = usize::from(u16_at(zip, at + 10)?);
    let offset = u32_at(zip, at + 16)?;
    if count == 0xFFFF || offset == 0xFFFF_FFFF {
        return None;
    }
    Some((count, usize::try_from(offset).ok()?))
}

fn entries(zip: &[u8]) -> Result<Vec<Entry<'_>>, ZipError> {
    let (count, mut at) = end_record(zip).ok_or(ZipError::Malformed)?;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let bad = || ZipError::Malformed;
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let name_len = usize::from(u16_at(zip, at + 28).ok_or_else(bad)?);
        let extra = usize::from(u16_at(zip, at + 30).ok_or_else(bad)?);
        let comment = usize::from(u16_at(zip, at + 32).ok_or_else(bad)?);
        let name = zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?;
        let made_by_unix = u16_at(zip, at + 4).ok_or_else(bad)? >> 8 == 3;
        let attrs = u32_at(zip, at + 38).ok_or_else(bad)?;
        out.push(Entry {
            name: std::str::from_utf8(name).map_err(|_| bad())?,
            method: u16_at(zip, at + 10).ok_or_else(bad)?,
            crc: u32_at(zip, at + 16).ok_or_else(bad)?,
            packed: usize::try_from(u32_at(zip, at + 20).ok_or_else(bad)?).map_err(|_| bad())?,
            size: u64::from(u32_at(zip, at + 24).ok_or_else(bad)?),
            mode: made_by_unix.then_some(attrs >> 16),
            local: usize::try_from(u32_at(zip, at + 42).ok_or_else(bad)?).map_err(|_| bad())?,
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// The entry's bytes, inflated and checked.
fn content(zip: &[u8], e: &Entry<'_>) -> Result<Vec<u8>, ZipError> {
    let bad = || ZipError::Malformed;
    if u32_at(zip, e.local) != Some(0x0403_4b50) {
        return Err(bad());
    }
    let name_len = usize::from(u16_at(zip, e.local + 26).ok_or_else(bad)?);
    let extra = usize::from(u16_at(zip, e.local + 28).ok_or_else(bad)?);
    let start = e.local + 30 + name_len + extra;
    let packed = zip.get(start..start + e.packed).ok_or_else(bad)?;
    let mut out = Vec::new();
    match e.method {
        0 => out.extend_from_slice(packed),
        8 => {
            // Never inflate past what the directory declared: a lying size cannot make a bomb.
            DeflateDecoder::new(packed)
                .take(e.size + 1)
                .read_to_end(&mut out)
                .map_err(|_| bad())?;
        }
        _ => return Err(bad()),
    }
    if out.len() as u64 != e.size || crc32fast::hash(&out) != e.crc {
        return Err(bad());
    }
    Ok(out)
}

/// A path inside `dest` for an archive name, or `None` for a name that escapes it (absolute, or with `..`).
fn inside(dest: &Path, name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    let clean = relative
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    clean.then(|| dest.join(relative))
}

/// Unpacks `zip` into `dest` (created if need be).
///
/// # Errors
/// [`ZipError`]: malformed, too big, or the disk refused.
pub(crate) fn extract(zip: &[u8], dest: &Path) -> Result<(), ZipError> {
    let list = entries(zip)?;
    if list.len() > MAX_ENTRIES || list.iter().map(|e| e.size).sum::<u64>() > MAX_UNPACKED {
        return Err(ZipError::TooBig);
    }
    std::fs::create_dir_all(dest).map_err(|_| ZipError::Io)?;
    for e in &list {
        let Some(path) = inside(dest, e.name) else {
            return Err(ZipError::Malformed);
        };
        if e.name.ends_with('/') {
            std::fs::create_dir_all(&path).map_err(|_| ZipError::Io)?;
            continue;
        }
        // A symbolic link is not unpacked: it could point outside the package.
        if e.mode.is_some_and(|m| m & 0o170_000 == 0o120_000) {
            continue;
        }
        let data = content(zip, e)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ZipError::Io)?;
        }
        std::fs::write(&path, &data).map_err(|_| ZipError::Io)?;
        set_mode(&path, e.mode);
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    let mode = mode.map_or(0o644, |m| (m & 0o777) | 0o400);
    // WHY: a failed chmod leaves the file readable but maybe not executable; the launch says so by name when it matters.
    if std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_err() {
        tracing::debug!("could not set the mode of {}", path.display());
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) {}

/// A stored (uncompressed) zip of `files`, for tests: name, bytes, unix mode.
#[cfg(test)]
pub(crate) fn build(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, mode) in files {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(data);
        let n = name.len() as u16;
        out.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        central.extend_from_slice(&[20, 3, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&n.to_le_bytes());
        central.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("doze-zip-{}-{name}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        dir
    }

    #[test]
    fn files_are_unpacked_with_their_modes() {
        let zip = build(&[
            ("a/b.txt", b"hello", 0o644),
            ("bootstrap", b"#!/bin/sh\n", 0o755),
        ]);
        let dir = scratch("modes");
        extract(&zip, &dir).unwrap();
        assert_eq!(std::fs::read(dir.join("a/b.txt")).unwrap(), b"hello");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("bootstrap"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn what_is_not_a_zip_or_escapes_is_refused() {
        let dir = scratch("bad");
        assert_eq!(
            extract(b"this is not a zip", &dir),
            Err(ZipError::Malformed)
        );
        let escape = build(&[("../x", b"1", 0o644)]);
        assert_eq!(extract(&escape, &dir), Err(ZipError::Malformed));
        let mut damaged = build(&[("a", b"hello", 0o644)]);
        if let Some(b) = damaged.get_mut(35) {
            *b ^= 0xff;
        }
        assert_eq!(extract(&damaged, &dir), Err(ZipError::Malformed));
        drop(std::fs::remove_dir_all(&dir));
    }
}
