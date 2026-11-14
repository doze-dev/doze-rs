//! Unpacking a verified archive into the cache (Go's `ensureDownload` tail): into `<dir>.tmp` first, its `bin/` moved
//! to the top if the archive nests it, the archive's checksum recorded beside it, then renamed into place, so a
//! half-unpacked build is never mistaken for a whole one. The `tar` crate keeps every entry inside the folder (Go's
//! check covered `..` names but would write through a symlink the archive made).

use std::io;
use std::path::{Path, PathBuf};

/// Where an unpacked build records the checksum of the archive it came from (Go's `.doze-archive-sha`).
pub const SHA_FILE: &str = ".doze-archive-sha";

/// The checksum a cached build was unpacked from, if it says.
pub fn recorded_sha(content_dir: &Path) -> Option<String> {
    let s = std::fs::read_to_string(content_dir.join(SHA_FILE)).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Whether a folder exists and has something in it.
pub fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some())
}

/// Unpacks the `.tar.gz` bytes into `content_dir`, which ends up with a non-empty `bin/`; returns that `bin/`.
pub fn unpack(archive: &[u8], content_dir: &Path, digest: &str) -> Result<PathBuf, String> {
    let tmp = content_dir.with_extension("tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    untar(archive, &tmp).map_err(|e| format!("extracting: {e}"))?;
    let bin = find_bin(&tmp).ok_or("the archive has no bin/ directory")?;
    if let Some(inner) = bin.parent().filter(|p| *p != tmp) {
        for e in std::fs::read_dir(inner)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            std::fs::rename(e.path(), tmp.join(e.file_name())).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(tmp.join(SHA_FILE), format!("{digest}\n")).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(content_dir);
    if let Some(parent) = content_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, content_dir).map_err(|e| e.to_string())?;
    let bin = content_dir.join("bin");
    if !has_files(&bin) {
        return Err("the build has no executables".into());
    }
    Ok(bin)
}

fn untar(archive: &[u8], dest: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dest)?;
    let mut a = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    a.set_preserve_permissions(true);
    a.set_overwrite(true);
    a.unpack(dest)
}

/// The first `bin` folder, breadth first (the top's own before a nested one).
fn find_bin(root: &Path) -> Option<PathBuf> {
    let mut queue = vec![root.to_path_buf()];
    while !queue.is_empty() {
        let mut next = Vec::new();
        for dir in queue {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut subdirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subdirs.sort();
            if let Some(b) = subdirs
                .iter()
                .find(|p| p.file_name().is_some_and(|n| n == "bin"))
            {
                return Some(b.clone());
            }
            next.extend(subdirs);
        }
        queue = next;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `.tar.gz` holding `files` (path, contents, executable).
    pub(crate) fn targz(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (path, body, exe) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(if *exe { 0o755 } else { 0o644 });
            h.set_cksum();
            b.append_data(&mut h, path, *body).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn a_nested_bin_comes_to_the_top_and_the_sha_is_kept() {
        let d = std::env::temp_dir().join(format!("doze-fetch-archive-{}", std::process::id()));
        let content = d.join("valkey/9.1.1-x");
        let a = targz(&[
            ("valkey-9/bin/valkey-server", b"#!", true),
            ("valkey-9/README", b"r", false),
        ]);
        let bin = unpack(&a, &content, "sha256:ab").unwrap();
        assert!(bin.join("valkey-server").exists() && content.join("README").exists());
        assert_eq!(recorded_sha(&content).as_deref(), Some("sha256:ab"));
        assert!(!content.with_extension("tmp").exists());
        let none = targz(&[("lib/x.so", b"x", false)]);
        assert!(
            unpack(&none, &d.join("other"), "sha256:1")
                .unwrap_err()
                .contains("no bin/")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Go's `TestADownloadCannotEscapeItsDirectory`: an entry naming a path outside is not written there.
    #[test]
    fn an_archive_cannot_write_outside_its_folder() {
        let d = std::env::temp_dir().join(format!("doze-fetch-escape-{}", std::process::id()));
        let content = d.join("cache/x/1-t");
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o755);
        // set_path refuses "..", so the name goes into the header's raw bytes as a hostile archive would have it.
        let name = b"../../escaped";
        if let Some(raw) = h.as_old_mut().name.get_mut(..name.len()) {
            raw.copy_from_slice(name);
        }
        h.set_cksum();
        b.append(&h, &b"x"[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(&mut h, "bin/ok", &b"y"[..]).unwrap();
        let a = b.into_inner().unwrap().finish().unwrap();
        let _ = unpack(&a, &content, "sha256:ab");
        assert!(!d.join("cache/escaped").exists() && !d.join("escaped").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
