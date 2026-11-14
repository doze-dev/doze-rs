//! The module registry: where it is, how its files are fetched (each host in turn, within one deadline), and this
//! machine's copies of them. Hosts are interchangeable: what is trusted is the signature, checked with the key built
//! into doze, so a second host is a mirror and never a risk.

pub(crate) mod http;
pub mod index;
pub mod latest;
pub mod meta;
pub mod select;
pub mod signed;
pub mod source;
pub(crate) mod yaml;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The registry doze uses unless told otherwise.
pub const DEFAULT_BASE: &str = "https://registry.doze-dev.com";

/// The registry's Cloudflare Pages host: the same signed files (the live test's target; an override while the
/// registry's hostname has no DNS answer, as on 2026-11-14).
pub const PAGES_BASE: &str = "https://doze-registry.pages.dev";

/// The variable that overrides where the registry is: one or more bases, comma separated, each `https://…`,
/// `file://<dir>` or a directory. Hosts are interchangeable (the signature is what is trusted, with the key built
/// in), so pointing it elsewhere can never make doze run something unsigned.
pub const OVERRIDE_VAR: &str = "DOZE_REGISTRY";

/// The bases `value` names (see [`OVERRIDE_VAR`]); empty parts are skipped.
pub fn parse_bases(value: &str) -> Vec<Base> {
    value
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| {
            if v.starts_with("https://") || v.starts_with("http://") {
                Base::Http(v.trim_end_matches('/').to_string())
            } else {
                Base::Dir(PathBuf::from(v.strip_prefix("file://").unwrap_or(v)))
            }
        })
        .collect()
}

/// The registry's bases: [`OVERRIDE_VAR`] when set and not empty, else [`DEFAULT_BASE`].
pub fn bases_from_env() -> Vec<Base> {
    let set = std::env::var(OVERRIDE_VAR)
        .map(|v| parse_bases(&v))
        .unwrap_or_default();
    if set.is_empty() {
        vec![Base::Http(DEFAULT_BASE.to_string())]
    } else {
        set
    }
}

/// The registry's pages on the website, for `--web`.
pub const WEB: &str = "https://doze-dev.com/registry";

/// Rule 5: the network gets this long, in all, when a command has to go out.
pub const BUDGET: Duration = Duration::from_secs(3);

/// Where a registry's files come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// A host, `https://…`.
    Http(String),
    /// A directory laid out as the registry (a mirror on disk; the tests' fixture).
    Dir(PathBuf),
}

impl Base {
    /// How the base is named in a message: the host, or the directory.
    pub fn name(&self) -> String {
        match self {
            Base::Http(url) => url
                .split_once("://")
                .map_or(url.as_str(), |(_, h)| h)
                .trim_end_matches('/')
                .to_string(),
            Base::Dir(d) => d.display().to_string(),
        }
    }
}

/// Which registry, and the key its files must be signed with.
#[derive(Debug, Clone)]
pub struct Config {
    pub bases: Vec<Base>,
    pub key: ed25519_dalek::VerifyingKey,
    pub budget: Duration,
}

/// A file, and which base gave it.
#[derive(Debug)]
pub struct Fetched {
    pub body: Vec<u8>,
    pub from: String,
    /// Bases that were tried first and did not answer, with why.
    pub skipped: Vec<(String, String)>,
}

/// Why a file could not be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// No base answered: each, and why.
    Unreachable(Vec<(String, String)>),
    /// A base answered that there is no such file.
    Missing { from: String },
}

impl FetchError {
    /// One line: `registry.doze-dev.com: no DNS answer; doze-registry.pages.dev: no answer within 3s`.
    pub fn reasons(&self) -> String {
        match self {
            FetchError::Unreachable(why) => why
                .iter()
                .map(|(b, w)| format!("{b}: {w}"))
                .collect::<Vec<_>>()
                .join("; "),
            FetchError::Missing { from } => format!("{from} has no such file"),
        }
    }
}

impl Config {
    /// GETs `path` (relative to the registry root) from each base in turn until one answers.
    pub fn get(&self, path: &str) -> Result<Fetched, FetchError> {
        let deadline = Instant::now() + self.budget;
        let mut skipped = Vec::new();
        for base in &self.bases {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                skipped.push((base.name(), "not tried: out of time".to_string()));
                continue;
            }
            match fetch(base, path, left) {
                Ok(body) if looks_like_html(&body) => {
                    return Err(FetchError::Missing { from: base.name() });
                }
                Ok(body) => {
                    return Ok(Fetched {
                        body,
                        from: base.name(),
                        skipped,
                    });
                }
                Err(http::Failure::Status(404 | 410)) => {
                    return Err(FetchError::Missing { from: base.name() });
                }
                Err(http::Failure::Status(s)) => {
                    skipped.push((base.name(), format!("answered {s}")))
                }
                Err(http::Failure::Unreachable(why)) => skipped.push((base.name(), why)),
            }
        }
        Err(FetchError::Unreachable(skipped))
    }
}

fn fetch(base: &Base, path: &str, left: Duration) -> Result<Vec<u8>, http::Failure> {
    match base {
        Base::Http(url) => http::get(&format!("{}/{path}", url.trim_end_matches('/')), left),
        Base::Dir(dir) if !dir.is_dir() => Err(http::Failure::Unreachable(
            "no registry in that directory".into(),
        )),
        Base::Dir(dir) => fs::read(dir.join(path)).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => http::Failure::Status(404),
            k => http::Failure::Unreachable(format!("cannot read it ({k})")),
        }),
    }
}

/// A static host answers a missing path with its HTML 404 page, sometimes with a success status (Go's
/// `looksLikeHTML`): that is "no such module", not a corrupt file.
fn looks_like_html(body: &[u8]) -> bool {
    let start = body
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(body.len());
    let head = body
        .get(start..start.saturating_add(14).min(body.len()))
        .unwrap_or_default();
    let head = String::from_utf8_lossy(head).to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

/// This machine's copies of registry files, under `<home>/registry`, laid out as the registry is.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(home: &Path) -> Cache {
        Cache {
            root: home.join("registry"),
        }
    }

    /// The file and how old it is, if this machine has it.
    pub fn read(&self, path: &str, now: SystemTime) -> Option<(String, Duration)> {
        let file = self.root.join(path);
        let text = fs::read_to_string(&file).ok()?;
        let modified = fs::metadata(&file).and_then(|m| m.modified()).ok()?;
        Some((text, now.duration_since(modified).unwrap_or_default()))
    }

    /// Keeps a file. Written whole or not at all (a temporary file, renamed into place).
    pub fn write(&self, path: &str, body: &[u8]) -> io::Result<()> {
        let file = self.root.join(path);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension(format!("tmp{}", std::process::id()));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &file)
    }

    /// The modules this machine has documentation for, for "did you mean".
    pub fn modules(&self, ns: &str) -> Vec<String> {
        fs::read_dir(self.root.join(ns))
            .map(|d| {
                d.filter_map(|e| e.ok()?.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_names_hosts_and_directories() {
        assert_eq!(
            parse_bases(" https://doze-registry.pages.dev/ ,file:///srv/reg,, /tmp/r"),
            vec![
                Base::Http(PAGES_BASE.into()),
                Base::Dir("/srv/reg".into()),
                Base::Dir("/tmp/r".into()),
            ]
        );
        assert!(parse_bases(" , ").is_empty());
    }

    /// Audit F1, minor 9: the cache keeps a meta.yaml as published, fields this doze does not read included, so a newer
    /// doze reading the same cache finds them.
    #[test]
    fn the_cache_keeps_the_published_bytes() {
        let home = std::env::temp_dir().join(format!("doze-fetch-cache-{}", std::process::id()));
        let c = Cache::new(&home);
        let body = "title: Valkey\nengine: valkey\nsomethingNew: {nested: [1, 2]}\nclient:\n  program: valkey-cli\n";
        c.write("doze/valkey/meta.yaml", body.as_bytes()).unwrap();
        let (back, _) = c.read("doze/valkey/meta.yaml", SystemTime::now()).unwrap();
        assert_eq!(back, body);
        assert!(super::meta::parse(&back).is_ok());
        let _ = fs::remove_dir_all(&home);
    }
}
