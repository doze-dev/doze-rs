//! The module archive: a gzipped tar with exactly one member, `bin/<engine>-plugin` (`.exe` on Windows), mode 0755,
//! and no timestamps or owners, so the same binary always gives the same bytes and the same sha256 (a rebuild of a
//! published version must hash the same or be refused).

use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};
use std::io::Write;

fn octal(field: &mut [u8], value: u64) {
    let digits = field.len().saturating_sub(1);
    let text = format!("{value:0digits$o}");
    let bytes = text.as_bytes();
    let start = bytes.len().saturating_sub(digits);
    for (slot, b) in field.iter_mut().zip(bytes.iter().skip(start)) {
        *slot = *b;
    }
}

/// A ustar header for a regular file.
fn header(name: &str, size: u64) -> Result<[u8; 512], String> {
    let mut h = [0u8; 512];
    let n = name.as_bytes();
    if n.len() > 100 {
        return Err(format!("{name}: a member name is at most 100 bytes"));
    }
    let put = |h: &mut [u8; 512], at: usize, bytes: &[u8]| {
        for (i, b) in bytes.iter().enumerate() {
            if let Some(slot) = h.get_mut(at + i) {
                *slot = *b;
            }
        }
    };
    put(&mut h, 0, n);
    let mut field = [0u8; 8];
    octal(&mut field, 0o755);
    put(&mut h, 100, &field);
    octal(&mut field, 0);
    put(&mut h, 108, &field);
    put(&mut h, 116, &field);
    let mut size_field = [0u8; 12];
    octal(&mut size_field, size);
    put(&mut h, 124, &size_field);
    let mut mtime = [0u8; 12];
    octal(&mut mtime, 0);
    put(&mut h, 136, &mtime);
    put(&mut h, 148, b"        ");
    put(&mut h, 156, b"0");
    put(&mut h, 257, b"ustar\x0000");
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    let mut chk = [0u8; 8];
    octal(&mut chk, sum);
    put(&mut h, 148, chk.get(..7).unwrap_or_default());
    Ok(h)
}

/// The archive bytes for `binary` as member `member`, and their sha256 (lowercase hex).
pub(crate) fn archive(member: &str, binary: &[u8]) -> Result<(Vec<u8>, String), String> {
    let mut tar = Vec::with_capacity(binary.len() + 2048);
    tar.extend_from_slice(&header(member, binary.len() as u64)?);
    tar.extend_from_slice(binary);
    let pad = (512 - binary.len() % 512) % 512;
    tar.extend(std::iter::repeat_n(0u8, pad + 1024));
    let mut gz = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    gz.write_all(&tar).map_err(|e| e.to_string())?;
    let bytes = gz.finish().map_err(|e| e.to_string())?;
    let sha = hex::encode(Sha256::digest(&bytes));
    Ok((bytes, sha))
}
