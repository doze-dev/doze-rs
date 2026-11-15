//! A zip archive read into memory (stored and deflated entries), for a bucket deployment's asset archives. Bounded: at most
//! `MAX_FILES` entries and `MAX_BYTES` unpacked, and a name that would leave the destination (`..`, an absolute path) is refused.

// fuzz:parser fuzz_unzip

use std::io::Read;

const MAX_FILES: usize = 100_000;
const MAX_BYTES: u64 = 1 << 30;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// The files of an archive: name and contents (directories left out).
///
/// # Errors
/// What is wrong with the archive.
pub(crate) fn files(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let bad = || "the archive is not a zip file it can read".to_owned();
    // The end-of-central-directory record is in the last 64 KiB plus its own 22 bytes.
    let from = zip.len().saturating_sub(65_557);
    let end = (from..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(zip, i) == Some(0x0605_4b50))
        .ok_or_else(bad)?;
    let count = usize::from(u16_at(zip, end + 10).ok_or_else(bad)?);
    let mut at = u32_at(zip, end + 16).ok_or_else(bad)? as usize;
    if count > MAX_FILES {
        return Err("the archive has too many files".into());
    }
    let mut out = Vec::new();
    let mut total = 0u64;
    for _ in 0..count {
        if u32_at(zip, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let packed = u32_at(zip, at + 20).ok_or_else(bad)? as usize;
        let size = u32_at(zip, at + 24).ok_or_else(bad)?;
        let name_len = usize::from(u16_at(zip, at + 28).ok_or_else(bad)?);
        let extra = usize::from(u16_at(zip, at + 30).ok_or_else(bad)?);
        let comment = usize::from(u16_at(zip, at + 32).ok_or_else(bad)?);
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?)
            .into_owned();
        at += 46 + name_len + extra + comment;
        if name.ends_with('/') {
            continue;
        }
        if name.starts_with('/') || name.split(['/', '\\']).any(|part| part == "..") {
            return Err(format!("the archive names a file outside it: {name}"));
        }
        total += u64::from(size);
        if total > MAX_BYTES {
            return Err("the archive unpacks to more than 1 GiB".into());
        }
        let local_name = usize::from(u16_at(zip, local + 26).ok_or_else(bad)?);
        let local_extra = usize::from(u16_at(zip, local + 28).ok_or_else(bad)?);
        let start = local + 30 + local_name + local_extra;
        let data = zip.get(start..start + packed).ok_or_else(bad)?;
        let contents = match method {
            0 => data.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize::try_from(size).unwrap_or(0).min(1 << 24));
                flate2::read::DeflateDecoder::new(data)
                    .take(u64::from(size) + 1)
                    .read_to_end(&mut v)
                    .map_err(|_| bad())?;
                v
            }
            _ => {
                return Err(format!(
                    "the archive uses a compression method it cannot read ({method})"
                ));
            }
        };
        out.push((name, contents));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::files;

    #[test]
    fn a_stored_archive_reads_back() {
        let zip = crate::zip::stored(&[("a.txt", b"hello"), ("dir/b.txt", b"world")]);
        let got = files(&zip).unwrap();
        assert_eq!(
            got,
            vec![
                ("a.txt".to_owned(), b"hello".to_vec()),
                ("dir/b.txt".to_owned(), b"world".to_vec())
            ]
        );
        assert!(files(b"not a zip").is_err());
        let evil = crate::zip::stored(&[("../x", b"no")]);
        assert!(files(&evil).is_err());
    }
}
