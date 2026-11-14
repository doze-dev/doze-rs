//! Engine builds from the doze-binaries mirror (Go's `binaries.Manager`): each engine's `index.yaml` maps a major to its
//! full version and lists each build's archive per platform with its checksum (and, when the mirror signs, a
//! signature). A build is cached at `<home>/<engine>/<full>-<triple>/bin` and checked against the lock's pin when there
//! is one, else against the manifest.

use crate::archive;
use crate::http::{self, ARCHIVE_LIMIT, Failure};
use crate::registry::signed;
use crate::registry::yaml::{self, YamlError, entries, sub, text};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the mirror is unless `DOZE_MIRROR` or `DOZE_<ENGINE>_MIRROR` says otherwise.
pub const DEFAULT_ROOT: &str = "https://github.com/doze-dev/doze-binaries/releases/download";

/// How long one archive may take.
pub const DOWNLOAD_BUDGET: Duration = Duration::from_secs(600);

/// One build's archive for one platform.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub sig: String,
}

/// One engine's part of a mirror manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineManifest {
    /// Major to full version.
    pub versions: BTreeMap<String, String>,
    /// Full version, then platform triple, to the archive.
    pub artifacts: BTreeMap<String, BTreeMap<String, Artifact>>,
}

// fuzz:parser fuzz_manifest
/// Reads a mirror manifest (`engines: {<engine>: {versions, artifacts}}`).
pub fn parse_manifest(src: &str) -> Result<BTreeMap<String, EngineManifest>, YamlError> {
    let h = yaml::mapping(src)?;
    let mut out = BTreeMap::new();
    for (engine, m) in sub(&h, "engines")?.into_iter().flat_map(entries) {
        let yaml_rust2::Yaml::Hash(m) = m else {
            continue;
        };
        let versions = sub(m, "versions")?
            .into_iter()
            .flat_map(entries)
            .filter_map(|(k, v)| yaml::scalar(v).map(|v| (k, v)))
            .collect();
        let mut artifacts = BTreeMap::new();
        for (full, by_triple) in sub(m, "artifacts")?.into_iter().flat_map(entries) {
            let yaml_rust2::Yaml::Hash(by_triple) = by_triple else {
                continue;
            };
            let arts = entries(by_triple)
                .filter_map(|(t, a)| {
                    let yaml_rust2::Yaml::Hash(a) = a else {
                        return None;
                    };
                    Some((
                        t,
                        Artifact {
                            url: text(a, "url"),
                            sha256: text(a, "sha256"),
                            sig: text(a, "sig"),
                        },
                    ))
                })
                .collect();
            artifacts.insert(full, arts);
        }
        out.insert(
            engine,
            EngineManifest {
                versions,
                artifacts,
            },
        );
    }
    Ok(out)
}

/// The mirror, for one doze home.
#[derive(Debug, Clone)]
pub struct Mirror {
    /// `DOZE_MIRROR` or [`DEFAULT_ROOT`]; `DOZE_<ENGINE>_MIRROR` overrides one engine.
    pub root: String,
    pub home: PathBuf,
    /// Set when the mirror signs its builds: each archive's signature must then match.
    pub key: Option<ed25519_dalek::VerifyingKey>,
}

impl Mirror {
    /// The mirror for `home`, as the environment sets it.
    pub fn from_env(home: &Path) -> Mirror {
        let root = std::env::var("DOZE_MIRROR").unwrap_or_else(|_| DEFAULT_ROOT.into());
        Mirror {
            root,
            home: home.into(),
            key: None,
        }
    }

    fn base(&self, engine: &str) -> String {
        if let Ok(v) = std::env::var(format!("DOZE_{}_MIRROR", engine.to_ascii_uppercase())) {
            return v;
        }
        format!("{}/{engine}", self.root.trim_end_matches('/'))
    }

    fn manifest(&self, engine: &str) -> Result<EngineManifest, String> {
        let url = format!("{}/index.yaml", self.base(engine).trim_end_matches('/'));
        let body = http::get(&url, DOWNLOAD_BUDGET)
            .map_err(|f| format!("fetching mirror manifest {url}: {}", said(&f)))?;
        let text = String::from_utf8_lossy(&body);
        let mut all =
            parse_manifest(&text).map_err(|e| format!("parsing mirror manifest: {}", e.message))?;
        all.remove(engine)
            .ok_or_else(|| format!("mirror has no engine \"{engine}\""))
    }

    /// The full version the mirror maps `major` of `engine` to.
    pub fn resolve_major(&self, engine: &str, major: &str) -> Result<String, String> {
        let m = self.manifest(engine)?;
        m.versions
            .get(major)
            .cloned()
            .ok_or_else(|| format!("mirror has no {engine} version for major \"{major}\""))
    }

    /// The cache folder of one build.
    pub fn content_dir(&self, engine: &str, full: &str, triple: &str) -> PathBuf {
        self.home.join(engine).join(format!("{full}-{triple}"))
    }

    /// Makes the build present and verified; its `bin/` and the archive's `sha256:<hex>`. `expected` is the lock's pin.
    pub fn ensure(
        &self,
        engine: &str,
        full: &str,
        triple: &str,
        expected: Option<&str>,
    ) -> Result<(PathBuf, String), String> {
        let content = self.content_dir(engine, full, triple);
        if archive::has_files(&content.join("bin")) {
            return cached(&content, &format!("{engine} {full} ({triple})"), expected);
        }
        let m = self.manifest(engine)?;
        let art = m
            .artifacts
            .get(full)
            .and_then(|a| a.get(triple))
            .ok_or_else(|| format!("mirror has no {engine} {full} artifact for {triple}"))?;
        let base = self.base(engine);
        let url = if art.url.contains("://") {
            local_only_from_local(&art.url, &base)?
        } else {
            format!(
                "{}/{}",
                base.trim_end_matches('/'),
                art.url.trim_start_matches('/')
            )
        };
        let what = format!("{engine} {full} ({triple})");
        let want = Want {
            manifest_sha: &art.sha256,
            sig: &art.sig,
            expected,
            key: self.key.as_ref(),
        };
        download(&url, &content, &what, &want)
    }
}

/// An absolute artifact URL as a manifest at `base` gives it: a `file://` one only from a manifest that is itself on
/// this machine (F1 audit 3, minor 3: a remote, unsigned manifest must not make doze read local files).
pub(crate) fn local_only_from_local(url: &str, base: &str) -> Result<String, String> {
    if url.starts_with("file://") && base.contains("://") && !base.starts_with("file://") {
        return Err(format!(
            "refusing {url}: a manifest from {base} may not point at a file on this machine"
        ));
    }
    Ok(url.to_string())
}

/// What a download is checked against.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Want<'a> {
    /// Lowercase hex from the manifest or index.
    pub(crate) manifest_sha: &'a str,
    pub(crate) sig: &'a str,
    /// `sha256:<hex>` from the lock: when set, it decides.
    pub(crate) expected: Option<&'a str>,
    pub(crate) key: Option<&'a ed25519_dalek::VerifyingKey>,
}

pub(crate) fn said(f: &Failure) -> String {
    match f {
        Failure::Unreachable(why) => why.clone(),
        Failure::Status(s) => format!("answered {s}"),
    }
}

/// A cached build, unless the lock pins another archive. What a child killed mid-unpack left beside it is swept when
/// nobody holds the build's lock (F1 audit 3: not only when the build is fetched again).
pub(crate) fn cached(
    content: &Path,
    what: &str,
    expected: Option<&str>,
) -> Result<(PathBuf, String), String> {
    if let Ok(Some(_held)) = doze_os::fs::try_lock(&lock_path(content)) {
        archive::clear_tmps(content);
    }
    cached_held(content, what, expected)
}

/// [`cached`], for a caller that holds the build's lock already.
fn cached_held(
    content: &Path,
    what: &str,
    expected: Option<&str>,
) -> Result<(PathBuf, String), String> {
    let digest = archive::recorded_sha(content).unwrap_or_default();
    if let Some(want) = expected.filter(|w| !digest.is_empty() && *w != digest) {
        return Err(format!(
            "cached {what} is {digest} but the lock pins {want} — remove {} to fetch it again",
            content.display()
        ));
    }
    Ok((content.join("bin"), digest))
}

/// Downloads, verifies (lock pin first, else manifest checksum; and the signature when there is a key), unpacks.
/// Parallel `__fetch` children may fill the same build: each takes the build's file lock (`<content>.lock`) first,
/// and one that finds the build there once it holds the lock takes it as cached.
pub(crate) fn download(
    url: &str,
    content: &Path,
    what: &str,
    want: &Want<'_>,
) -> Result<(PathBuf, String), String> {
    let _held = lock_build(content)?;
    if archive::has_files(&content.join("bin")) {
        archive::clear_tmps(content);
        return cached_held(content, what, want.expected);
    }
    let bytes = http::get_up_to(url, DOWNLOAD_BUDGET, ARCHIVE_LIMIT)
        .map_err(|f| format!("downloading {url}: {}", said(&f)))?;
    let got = signed::sha256_hex(&bytes);
    let digest = format!("sha256:{got}");
    match want.expected {
        Some(pin) if pin != digest => {
            return Err(format!(
                "checksum mismatch for {what}: locked {pin}, got {digest}"
            ));
        }
        Some(_) => {}
        None if want.manifest_sha.is_empty() => {
            return Err(format!("no checksum available to verify {what}"));
        }
        None if !want.manifest_sha.eq_ignore_ascii_case(&got) => {
            return Err(format!(
                "checksum mismatch for {what}: manifest {}, got {got}",
                want.manifest_sha
            ));
        }
        None => {}
    }
    if let Some(key) = want.key {
        signed::verify_hex(key, &got, want.sig)
            .map_err(|r| format!("signature check failed for {what}: {r}"))?;
    }
    let bin = archive::unpack(&bytes, content, &digest)?;
    Ok((bin, digest))
}

/// The cross-process lock on one cache build, held while it is checked, downloaded and unpacked.
pub(crate) fn lock_build(content: &Path) -> Result<doze_os::fs::Lock, String> {
    let path = lock_path(content);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    doze_os::fs::lock(&path).map_err(|e| format!("locking {}: {e}", path.display()))
}

fn lock_path(content: &Path) -> PathBuf {
    let mut name = content.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
