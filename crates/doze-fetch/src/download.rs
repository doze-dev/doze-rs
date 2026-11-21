//! A service's `download = "<url>"` (Go's `binaries.EnsureDownload`): anything a stack runs that is not in the doze
//! registry, a `.tar.gz` or a single executable, fetched once into `<home>/downloads/<hash of the URL>` and pinned in
//! doze.lock by the checksum of what the URL served. A later fetch, here or on another machine with the same lock, must
//! bring the same bytes: a change at the source is refused before anything is unpacked, and a cache entry is checked
//! against the pin too (the archive is gone by then, so against the digest recorded when it was unpacked).
//!
//! What runs is the archive's root: its single top-level folder if it has one (`tool-1.0/…`), else where it was
//! unpacked. A bare executable gets a folder of its own, named after the URL's last segment.
//!
//! Differs from Go: a `.zip` is refused with a word (Go unpacked it); the `zip` reader would add to every `doze` for a
//! format no example uses. Bytes are held in memory up to the archive limit (Go streamed them to disk).

use crate::archive;
use crate::http;
use crate::mirror::{DOWNLOAD_BUDGET, lock_build, said};
use crate::registry::signed;
use std::path::{Path, PathBuf};

/// Where a URL is cached under the doze home.
pub fn content_dir(home: &Path, url: &str) -> PathBuf {
    let key = signed::sha256_hex(url.as_bytes());
    home.join("downloads").join(key.get(..16).unwrap_or(&key))
}

/// Makes what `url` serves present and returns the folder a command runs in and the digest (`sha256:<hex>`).
/// `expected` is the lock's pin; empty on first use, when the caller pins whatever arrived.
pub fn ensure(home: &Path, url: &str, expected: Option<&str>) -> Result<(PathBuf, String), String> {
    let content = content_dir(home, url);
    let _held = lock_build(&content)?;
    if let Some(have) = archive::recorded_sha(&content) {
        if let Some(pin) = expected.filter(|p| *p != have) {
            return Err(format!(
                "the download {url} is cached as {have} but doze.lock pins {pin} — it changed at the source, or the \
                 lock is wrong; remove {} to fetch it again",
                content.display()
            ));
        }
        return Ok((root(&content), have));
    }
    let bytes = http::get_up_to(url, DOWNLOAD_BUDGET, http::ARCHIVE_LIMIT)
        .map_err(|f| format!("downloading {url}: {}", said(&f)))?;
    let digest = format!("sha256:{}", signed::sha256_hex(&bytes));
    if let Some(pin) = expected.filter(|p| *p != digest) {
        return Err(format!(
            "checksum mismatch for {url}: doze.lock pins {pin}, the server sent {digest} — nothing was unpacked"
        ));
    }
    archive::clear_tmps(&content);
    let mut tmp = content.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let r = unpack_into(&bytes, url, &tmp, &digest).and_then(|()| {
        let _ = std::fs::remove_dir_all(&content);
        std::fs::rename(&tmp, &content).map_err(|e| e.to_string())
    });
    if let Err(e) = r {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("unpacking {url}: {e}"));
    }
    Ok((root(&content), digest))
}

fn unpack_into(bytes: &[u8], url: &str, tmp: &Path, digest: &str) -> Result<(), String> {
    let body = tmp.join("root");
    std::fs::create_dir_all(&body).map_err(|e| e.to_string())?;
    match bytes {
        [0x1f, 0x8b, ..] => archive::untar_into(bytes, &body).map_err(|e| e.to_string())?,
        [b'P', b'K', 3, 4, ..] => {
            return Err(
                "a .zip archive: doze unpacks .tar.gz archives and single executables".into(),
            );
        }
        _ => {
            let name = url
                .split(['?', '#'])
                .next()
                .and_then(|u| u.rsplit(['/', '\\']).next())
                .filter(|n| !n.is_empty() && *n != "." && *n != "..")
                .unwrap_or("download");
            doze_os::fs::write_executable(&body.join(name), bytes).map_err(|e| e.to_string())?;
        }
    }
    doze_os::fs::drop_shared_write(tmp).map_err(|e| e.to_string())?;
    std::fs::write(tmp.join(archive::SHA_FILE), format!("{digest}\n")).map_err(|e| e.to_string())
}

/// Where a command runs: the archive's single top-level folder, else its unpacked root.
fn root(content: &Path) -> PathBuf {
    let body = content.join("root");
    let entries: Vec<PathBuf> = std::fs::read_dir(&body)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    match entries.as_slice() {
        [one] if std::fs::symlink_metadata(one).is_ok_and(|m| m.is_dir()) => one.clone(),
        _ => body,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("doze-dl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn served(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let f = dir.join(name);
        std::fs::write(&f, bytes).unwrap();
        format!("file://{}", f.display())
    }

    #[test]
    fn an_archive_is_unpacked_once_runs_from_its_folder_and_a_changed_one_is_refused() {
        let h = home("tgz");
        let a = archive::tests::targz(&[("tool-1.0/bin/serve", b"#!/bin/sh\n", true)]);
        let url = served(&h, "tool.tgz", &a);
        let (dir, digest) = ensure(&h, &url, None).unwrap();
        assert!(
            dir.ends_with("tool-1.0") && dir.join("bin/serve").exists(),
            "{}",
            dir.display()
        );
        assert!(digest.starts_with("sha256:"));
        // Cached: the same folder, the pin satisfied.
        assert_eq!(ensure(&h, &url, Some(&digest)).unwrap().0, dir);
        // Another machine with the same lock, the archive replaced at the source.
        let other = home("tgz-other");
        let b = archive::tests::targz(&[("tool-1.0/bin/serve", b"tampered", true)]);
        let url = served(&other, "tool.tgz", &b);
        let e = ensure(&other, &url, Some(&digest)).unwrap_err();
        assert!(
            e.contains("checksum mismatch") && e.contains("nothing was unpacked"),
            "{e}"
        );
        assert!(!content_dir(&other, &url).exists(), "nothing was unpacked");
        // A cache entry pinned to something else is refused too.
        let e = ensure(&h, &served(&h, "tool.tgz", &a), Some("sha256:00")).unwrap_err();
        assert!(e.contains("doze.lock pins sha256:00"), "{e}");
        let _ = std::fs::remove_dir_all(&h);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn a_bare_executable_gets_a_folder_and_a_zip_is_refused() {
        let h = home("bare");
        let url = served(&h, "mytool", b"#!/bin/sh\necho hi\n");
        let (dir, _) = ensure(&h, &url, None).unwrap();
        let tool = dir.join("mytool");
        assert!(tool.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&tool).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "{mode:o}");
        }
        let e = ensure(&h, &served(&h, "x.zip", b"PK\x03\x04rest"), None).unwrap_err();
        assert!(e.contains(".zip"), "{e}");
        let _ = std::fs::remove_dir_all(&h);
    }
}
