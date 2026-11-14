//! PostgreSQL 14 to 17 find control files only in the toolchain's own `share/extension`, so a bundle is copied into the
//! toolchain (shared by every project on that version), additively and under a lock. Versioned files
//! (`vector--0.8.7.sql`) sit beside each other; an unversioned one (`vector.control`, `vector.so`) belongs to the newest
//! version installed. A ledger records who placed each file, so a file doze did not write is never replaced, another
//! extension's never taken over, and an older bundle never replaces a newer one's. Go's rules and Go's ledger file, so
//! a toolchain either installed into reads the same; better than Go, its `source` bundles go through the same rules
//! (Go unpacked them over the toolchain with no record, modules plan bug 4).

use crate::bundle_read::{Ledger, Manifest, Owner, compare};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

const LEDGER: &str = ".doze-pgext.json";
const LOCK: &str = ".doze-pgext.lock";

/// Where a toolchain keeps extensions: its `sharedir` and `pkglibdir`.
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    pub share: PathBuf,
    pub pkglib: PathBuf,
}

fn io(what: &str, p: &Path) -> impl FnOnce(std::io::Error) -> String {
    let at = format!("{what} {}", p.display());
    move |e| format!("{at}: {e}")
}

/// A file's fingerprint: its SHA-256 in hex, or `link:<target>`; `None` when there is no file.
fn sum(path: &Path) -> Result<Option<String>, String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(None);
    };
    if meta.file_type().is_symlink() {
        let t = std::fs::read_link(path).map_err(io("reading", path))?;
        return Ok(Some(format!("link:{}", t.to_string_lossy())));
    }
    let bytes = std::fs::read(path).map_err(io("reading", path))?;
    Ok(Some(hex::encode(Sha256::digest(&bytes))))
}

/// A library the bundle carries for its own module (`lib/libssl.so.3`, `lib/libgeos.dylib`), not the module itself:
/// one the toolchain or another bundle has already is kept, and serves both.
fn vendored(key: &str) -> bool {
    let Some(base) = key.strip_prefix("lib/") else {
        return false;
    };
    !base.contains('/')
        && base.starts_with("lib")
        && (base.contains(".so") || base.ends_with(".dylib"))
}

/// What to do with one file, by the ledger's rules: `Ok(true)` to write it, `Ok(false)` to leave the one there.
fn decide(
    key: &str,
    dest: &Path,
    have: Option<&str>,
    sum: &str,
    m: &Manifest,
    owner: Option<&Owner>,
) -> Result<bool, String> {
    let Some(have) = have else { return Ok(true) };
    if have != sum && vendored(key) {
        return Ok(false);
    }
    let newer = |o: &Owner| compare(&m.version, &o.version) == Ordering::Greater;
    match owner {
        _ if have == sum => Ok(false),
        Some(o) if o.sha256 == have && o.ext != m.name => Err(format!(
            "{} belongs to extension {} {}; not overwriting it with {}'s",
            dest.display(),
            o.ext,
            o.version,
            m.name
        )),
        Some(o) if o.sha256 == have => Ok(newer(o)),
        _ => Err(format!(
            "{} already exists with different content and doze did not put it there; not overwriting it",
            dest.display()
        )),
    }
}

/// Places one bundle file at `dest`; whether the ledger changed.
fn place(
    src: &Path,
    dest: &Path,
    key: &str,
    m: &Manifest,
    files: &mut Ledger,
) -> Result<bool, String> {
    let fingerprint = sum(src)?.ok_or_else(|| format!("{} vanished", src.display()))?;
    let have = sum(dest)?;
    let owner = files.files.get(key);
    let mine = Owner {
        ext: m.name.clone(),
        version: m.version.clone(),
        sha256: fingerprint.clone(),
    };
    if have.as_deref() == Some(fingerprint.as_str()) {
        // Already there: record it as this bundle's if nobody owns it, or if this version is newer.
        let claim = owner.is_none_or(|o| compare(&m.version, &o.version) == Ordering::Greater);
        if claim {
            files.files.insert(key.to_owned(), mine);
        }
        return Ok(claim);
    }
    if !decide(key, dest, have.as_deref(), &fingerprint, m, owner)? {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io("creating", parent))?;
    }
    let tmp = PathBuf::from(format!("{}.doze-tmp", dest.display()));
    let _ = std::fs::remove_file(&tmp);
    let written = match fingerprint.strip_prefix("link:") {
        Some(target) => doze_os::fs::symlink(Path::new(target), &tmp),
        None => std::fs::copy(src, &tmp).map(drop),
    }
    // A rename over the old file: a server that loaded the library keeps its old copy.
    .and_then(|()| std::fs::rename(&tmp, dest));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("placing {}: {e}", dest.display()));
    }
    files.files.insert(key.to_owned(), mine);
    Ok(true)
}

/// Every file under `dir`, as paths relative to it; none when it does not exist.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        let at = dir.join(&rel);
        let Ok(list) = std::fs::read_dir(&at) else {
            continue;
        };
        for e in list {
            let e = e.map_err(io("listing", &at))?;
            let ty = e.file_type().map_err(io("reading", &e.path()))?;
            let name = rel.join(e.file_name());
            if ty.is_dir() {
                todo.push(name);
            } else {
                out.push(name);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Installs the bundle at `root` into `tree`, under the ledger's lock; what changed, for the log.
pub(crate) fn install(tree: &Tree, root: &Path, m: &Manifest) -> Result<Vec<String>, String> {
    let ext_dir = tree.share.join("extension");
    std::fs::create_dir_all(&ext_dir).map_err(io("creating", &ext_dir))?;
    let _held = doze_os::fs::lock(&ext_dir.join(LOCK)).map_err(io("locking", &ext_dir))?;
    let path = ext_dir.join(LEDGER);
    let mut ledger = match std::fs::read_to_string(&path) {
        Ok(text) => Ledger::parse(&text).map_err(|e| format!("reading {}: {e}", path.display()))?,
        Err(_) => Ledger::default(),
    };
    let root_key = root.to_string_lossy().into_owned();
    if ledger.bundles.get(&root_key) == Some(&true) {
        return Ok(vec![]);
    }
    let mut placed = Vec::new();
    let parts = [
        (
            root.join("share").join("extension"),
            ext_dir.clone(),
            "extension/",
        ),
        (root.join("lib"), tree.pkglib.clone(), "lib/"),
    ];
    let mut result = Ok(());
    'all: for (from, to, prefix) in parts {
        for rel in walk(&from)? {
            let key = format!("{prefix}{}", rel.to_string_lossy().replace('\\', "/"));
            match place(&from.join(&rel), &to.join(&rel), &key, m, &mut ledger) {
                Ok(true) => placed.push(key),
                Ok(false) => {}
                Err(e) => {
                    result = Err(e);
                    break 'all;
                }
            }
        }
    }
    if result.is_ok() {
        ledger.bundles.insert(root_key, true);
    }
    std::fs::write(&path, ledger.encode()).map_err(io("writing", &path))?;
    result.map_err(|e| format!("installing extension {} {}: {e}", m.name, m.version))?;
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dz-ledger-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn bundle(dir: &Path, version: &str, so: &str) -> Manifest {
        let ext = dir.join("share/extension");
        std::fs::create_dir_all(&ext).unwrap();
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(
            ext.join("x.control"),
            format!("default_version = '{version}'"),
        )
        .unwrap();
        std::fs::write(ext.join(format!("x--{version}.sql")), version).unwrap();
        std::fs::write(dir.join("lib/x.so"), so).unwrap();
        Manifest {
            name: "x".into(),
            version: version.into(),
            ..Manifest::default()
        }
    }

    #[test]
    fn newer_wins_older_adds_and_a_strangers_file_is_left() {
        let d = tmp("rules");
        let tree = Tree {
            share: d.join("tc/share"),
            pkglib: d.join("tc/lib"),
        };
        let (old, new) = (d.join("b1"), d.join("b2"));
        let m1 = bundle(&old, "1.0", "one");
        let m2 = bundle(&new, "1.1", "two");
        install(&tree, &new, &m2).unwrap();
        install(&tree, &old, &m1).unwrap();
        let read = |p: &str| std::fs::read_to_string(d.join("tc").join(p)).unwrap();
        // The newer bundle's unversioned files stay; the older one's versioned script is added beside.
        assert_eq!(read("lib/x.so"), "two");
        assert_eq!(read("share/extension/x--1.0.sql"), "1.0");
        assert!(read("share/extension/x.control").contains("1.1"));
        // A file doze never placed is not overwritten.
        std::fs::write(tree.pkglib.join("y.so"), "theirs").unwrap();
        let other = d.join("b3");
        let mut m3 = bundle(&other, "2.0", "three");
        std::fs::write(other.join("lib/y.so"), "mine").unwrap();
        m3.name = "x".into();
        let e = install(&tree, &other, &m3).unwrap_err();
        assert!(e.contains("did not put it there"), "{e}");
        assert_eq!(read("lib/y.so"), "theirs");
        // Another extension's file is never taken over.
        let z = d.join("b4");
        let mut mz = bundle(&z, "9.0", "zz");
        mz.name = "z".into();
        let e = install(&tree, &z, &mz).unwrap_err();
        assert!(e.contains("belongs to extension x"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vendored_libraries_are_shared() {
        assert!(vendored("lib/libssl.so.3") && vendored("lib/libgeos.dylib"));
        assert!(!vendored("lib/vector.so") && !vendored("extension/libx.so"));
    }
}
