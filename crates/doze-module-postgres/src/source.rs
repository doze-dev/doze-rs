//! A `source` extension bundle: an archive on disk, hashed, held to its declared `sha256` and to its doze.lock pin, and
//! unpacked once into a folder named by its hash beside the instance's data (Go unpacked it straight over the shared
//! toolchain, with no checksum, pin or record: modules plan bug 4).

use crate::bundle::{control_version, failed};
use crate::bundle_read::Manifest;
use crate::spec::{Ext, PostgresSpec};
use crate::{ext, tarball};
use doze_module::proto::msg::run::Pin;
use doze_module::{Cx, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The lock engine a `source` bundle is pinned under.
pub(crate) fn source_engine(name: &str) -> String {
    format!("pgext-source-{name}")
}

/// A `source` archive: hashed, held to its declared `sha256` and to the lock's pin, and unpacked once into a folder
/// named by its hash beside the instance's data.
pub(crate) fn source_bundle(
    cx: &Cx<PostgresSpec>,
    e: &Ext,
    pins: &[Pin],
    target: &str,
) -> Result<(PathBuf, Pin, Manifest)> {
    let src = e.source.clone().unwrap_or_default();
    let path = ext::source_path(&cx.instance.dir, &src);
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    if size > tarball::MAX_BYTES {
        return Err(failed(format!(
            "{} is larger than a bundle may be ({} MiB)",
            path.display(),
            tarball::MAX_BYTES >> 20
        )));
    }
    let bytes =
        std::fs::read(&path).map_err(|err| failed(format!("reading {}: {err}", path.display())))?;
    let hex = hex::encode(Sha256::digest(&bytes));
    let digest = format!("sha256:{hex}");
    if let Some(want) = &e.sha256
        && !want.eq_ignore_ascii_case(&hex)
    {
        return Err(failed(format!(
            "{} has sha256 {hex}, but doze.hcl declares {want}",
            path.display()
        )));
    }
    let engine = source_engine(&e.name);
    if let Some(p) = pins.iter().find(|p| p.engine == engine && p.spec == src)
        && p.resolved != digest
    {
        return Err(failed(format!(
            "{} changed since doze.lock pinned it ({} then, {digest} now); if the change is yours, update the lock",
            path.display(),
            p.resolved
        )));
    }
    let root = unpacked(&cx.instance.data_dir, &e.name, &hex, &bytes)?;
    if control_version(&root, &e.name).is_none() {
        return Err(failed(format!(
            "{} has no share/extension/{}.control",
            path.display(),
            e.name
        )));
    }
    let pin = Pin {
        engine,
        spec: src,
        resolved: digest.clone(),
        hashes: BTreeMap::from([(target.to_owned(), digest)]),
    };
    let m = crate::bundle::manifest(&root, &e.name);
    prune_others(&root, &e.name);
    Ok((root, pin, m))
}

/// The instance's earlier unpackings of `name` (other hashes) are removed once the declared one is in place: the
/// server is stopped while doze resolves, and 14 to 17 copied what they need into the toolchain.
fn prune_others(root: &Path, name: &str) {
    let (Some(base), Some(keep)) = (root.parent(), root.file_name()) else {
        return;
    };
    let Ok(list) = std::fs::read_dir(base) else {
        return;
    };
    let prefix = format!("{name}-");
    for e in list.filter_map(std::result::Result::ok) {
        let n = e.file_name();
        let hash = n.to_str().and_then(|s| s.strip_prefix(&prefix));
        let ours = hash.is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        if ours && n != keep {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Unpacks an archive once into `<data>.ext/<name>-<hash>` beside the data directory: made aside and renamed in, so a
/// folder there is always whole.
fn unpacked(data_dir: &str, name: &str, hex: &str, bytes: &[u8]) -> Result<PathBuf> {
    let data = Path::new(data_dir);
    let base = PathBuf::from(format!("{}.ext", data.display()));
    let root = base.join(format!("{name}-{}", hex.get(..16).unwrap_or(hex)));
    if root.join("share").is_dir() {
        return Ok(root);
    }
    let items = tarball::read(bytes)
        .map_err(|e| failed(format!("extension {name}: the source bundle: {e}")))?;
    let staging = base.join(format!(".{name}-{}.part", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let wrote = write_items(&staging, &items).and_then(|()| {
        std::fs::rename(&staging, &root).map_err(|e| format!("placing {}: {e}", root.display()))
    });
    let _ = std::fs::remove_dir_all(&staging);
    match wrote {
        Err(_) if root.join("share").is_dir() => Ok(root),
        Err(e) => Err(failed(e)),
        Ok(()) => Ok(root),
    }
}

fn write_items(dir: &Path, items: &[tarball::Item]) -> std::result::Result<(), String> {
    for it in items {
        let dest = dir.join(&it.path);
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("creating {}: {e}", p.display()))?;
        }
        let done = match &it.body {
            tarball::Body::File { data, exec: true } => doze_os::fs::write_executable(&dest, data),
            tarball::Body::File { data, .. } => std::fs::write(&dest, data),
            tarball::Body::Link(t) => doze_os::fs::symlink(Path::new(t), &dest),
        };
        done.map_err(|e| format!("writing {}: {e}", dest.display()))?;
    }
    Ok(())
}
