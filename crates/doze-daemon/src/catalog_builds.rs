//! Builds of one module type per major: two instances of a type on majors no single build serves each run on a build
//! of their own, as `type@major` in the pool.

use super::{Catalog, Manifest};

impl Catalog {
    /// `first` when it serves `version`'s major; else the build found for that major (looked for once), or `first`
    /// again when there is none (doze-config then refuses the version with `first`'s list).
    pub(super) fn another_build(&self, ty: &str, version: &str, first: Manifest) -> Manifest {
        let major = major_of(version);
        if serves(&first, major) {
            return first;
        }
        let key = format!("{ty}@{major}");
        if let Some(m) = self.manifest(&key) {
            return m;
        }
        if crate::lock(&self.failed).contains_key(&key) {
            return first;
        }
        match self.resolve(&key, ty, version) {
            Ok(m) if serves(&m, major) => {
                crate::lock(&self.manifests).insert(key, m.clone());
                m
            }
            Ok(_) => {
                crate::lock(&self.failed).insert(key, format!("no {ty} build serves {major}"));
                first
            }
            Err(e) => {
                crate::lock(&self.failed).insert(key, e);
                first
            }
        }
    }
}

/// The major of a declared version (`"17.2"` → `"17"`; empty for none).
pub(super) fn major_of(version: &str) -> &str {
    version.split('.').next().unwrap_or_default()
}

/// Whether a build serves `major` (a versionless module, or no version asked, serves any).
fn serves(m: &Manifest, major: &str) -> bool {
    major.is_empty() || m.versions.is_empty() || m.versions.iter().any(|v| v == major)
}

impl Catalog {
    /// Where type `ty` at `version` runs (its pool key and manifest key): the type's first build, or the build found for
    /// this major when the first does not serve it.
    pub fn key(&self, ty: &str, version: &str) -> String {
        let major = major_of(version);
        let ms = crate::lock(&self.manifests);
        match ms.get(ty) {
            Some(m) if !serves(m, major) => {
                let k = format!("{ty}@{major}");
                if ms.contains_key(&k) {
                    k
                } else {
                    ty.to_string()
                }
            }
            _ => ty.to_string(),
        }
    }

    /// The manifest of the build `ty` at `version` runs on, once looked up.
    pub fn manifest_for(&self, ty: &str, version: &str) -> Option<Manifest> {
        self.manifest(&self.key(ty, version))
    }
}
