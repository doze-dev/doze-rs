//! `doze __fetch`: every download runs in a short child process, never in the daemon (its HTTPS client, its TLS
//! roots and its memory go when the child exits). The daemon writes one [`Request`] as a JSON line to the child's
//! stdin and reads one [`Reply`] line from its stdout; [`ChildDownloads`] is that host side, serving the module
//! protocol's `fetch.resolve_major` and `fetch.ensure`.

use crate::lock::Lock;
use crate::mirror::{self, Mirror};
use crate::modules::Modules;
use crate::registry::source::Source;
use doze_module_host::services::Downloads;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::run::{Ensure, Ensured};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the child is asked to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// The full version the mirror maps `major` of `engine` to.
    ResolveMajor { engine: String, major: String },
    /// An exact engine build, present and verified.
    Ensure {
        engine: String,
        full: String,
        target: String,
        expected_sha: Option<String>,
    },
    /// A module build for the engine `majors`, pinned in the lock at `lock` (saved only when `persist`).
    Module {
        source: String,
        majors: Vec<String>,
        lock: Option<PathBuf>,
        persist: bool,
    },
}

/// What the child answers: `error` when it failed, else the fields its request fills.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bin_dir: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub digest: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exe: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

impl Reply {
    fn failed(e: String) -> Reply {
        Reply {
            error: Some(e),
            ..Reply::default()
        }
    }
}

/// Where the child fetches from and keeps what it fetched.
#[derive(Debug, Clone)]
pub struct Place {
    pub mirror: Mirror,
    pub modules: Modules,
}

/// Does what `req` asks (blocking: this is the child's whole life).
pub fn serve(req: &Request, place: &Place) -> Reply {
    // What a module asks for becomes cache folder names: plain names only, never a path.
    let names: Vec<&str> = match req {
        Request::ResolveMajor { engine, major } => vec![engine, major],
        Request::Ensure {
            engine,
            full,
            target,
            ..
        } => vec![engine, full, target],
        Request::Module { .. } => Vec::new(),
    };
    if let Some(bad) = names.iter().find(|n| !crate::lock::plain(n)) {
        return Reply::failed(format!("refused: {bad:?} is not a plain name"));
    }
    if let Request::Ensure { target, .. } = req
        && !known_triple(target, &place.modules.triple)
    {
        return Reply::failed(format!(
            "refused: {target:?} is not a platform doze builds for"
        ));
    }
    match req {
        Request::ResolveMajor { engine, major } => {
            match place.mirror.resolve_major(engine, major) {
                Ok(version) => Reply {
                    version,
                    ..Reply::default()
                },
                Err(e) => Reply::failed(e),
            }
        }
        Request::Ensure {
            engine,
            full,
            target,
            expected_sha,
        } => match place
            .mirror
            .ensure(engine, full, target, expected_sha.as_deref())
        {
            Ok((bin, digest)) => Reply {
                bin_dir: bin.display().to_string(),
                digest,
                ..Reply::default()
            },
            Err(e) => Reply::failed(e),
        },
        Request::Module {
            source,
            majors,
            lock,
            persist,
        } => module(place, source, majors, lock.as_deref(), *persist),
    }
}

/// The platforms builds are published for, and this machine's own (audit F1, M1: a module names the build it wants,
/// and only a real platform may become a cache folder).
const TRIPLES: [&str; 8] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-unknown-linux-musl",
    "aarch64-pc-windows-msvc",
    "x86_64-pc-windows-msvc",
];

fn known_triple(t: &str, here: &str) -> bool {
    t == here || TRIPLES.contains(&t)
}

fn module(
    place: &Place,
    source: &str,
    majors: &[String],
    lock: Option<&Path>,
    persist: bool,
) -> Reply {
    let Some(src) = Source::parse(source) else {
        return Reply::failed(format!(
            "invalid module source {source:?}: want <namespace>/<name>"
        ));
    };
    let mut l = match lock.map(Lock::load) {
        Some(Ok(l)) => l,
        Some(Err(e)) => return Reply::failed(e),
        None => Lock::default(),
    };
    let r = match place.modules.resolve(&src, majors, &mut l) {
        Ok(r) => r,
        Err(e) => return Reply::failed(e),
    };
    let mut notes: Vec<String> = lock
        .and_then(|p| newer_lock(p, &l, &place.modules.doze))
        .into_iter()
        .collect();
    notes.extend(r.notes);
    if let (Some(path), true, true) = (lock, persist, l.dirty) {
        match save_pin(place, path, &src, l.modules.get(&src.to_string())) {
            Ok(true) => notes.push(format!(
                "pinned {src} {} in doze.lock — commit the lockfile so your team and CI get this exact build",
                r.version
            )),
            Ok(false) => {}
            Err(e) => notes.push(format!("could not write {}: {e}", path.display())),
        }
    }
    Reply {
        version: r.version,
        exe: r.exe.display().to_string(),
        notes,
        ..Reply::default()
    }
}

/// Go's `loadLock` warning: the lock was written by a newer doze (said with each resolve that reads it).
fn newer_lock(path: &Path, l: &Lock, doze: &str) -> Option<String> {
    use crate::registry::select::{compare_versions, is_doze_version};
    let this = doze.trim_start_matches('v');
    let wrote = l.doze.trim_start_matches('v');
    (is_doze_version(this) && is_doze_version(wrote) && compare_versions(wrote, this).is_gt()).then(|| {
        let file = path.file_name().map_or_else(|| "doze.lock".into(), |f| f.to_string_lossy());
        format!("{file} was written by doze {wrote}, newer than this doze ({this}); if a module will not start, update doze")
    })
}

/// Writes this module's pin into the lock at `path`. Children for other modules may be saving the same file: under a
/// lock in the doze home (never beside the project's files), the file is read again and only this pin is changed in
/// it, so no other child's pin is lost (audit F1, M3). Whether the module was not pinned there before.
fn save_pin(
    place: &Place,
    path: &Path,
    src: &Source,
    pin: Option<&crate::lock::Pin>,
) -> Result<bool, String> {
    let Some(pin) = pin else {
        return Ok(false);
    };
    let locks = place.modules.home.join("locks");
    std::fs::create_dir_all(&locks)
        .map_err(|e| format!("taking the doze.lock lock in {}: {e}", locks.display()))?;
    let key = crate::registry::signed::sha256_hex(path.to_string_lossy().as_bytes());
    let _held =
        doze_os::fs::lock(&locks.join(format!("lock-{}.lock", key.get(..16).unwrap_or(&key))))
            .map_err(|e| e.to_string())?;
    let mut now = Lock::load(path)?;
    let new = !now.modules.contains_key(&src.to_string());
    now.record_module(&src.to_string(), pin.clone());
    now.save(path, Some(&place.modules.doze))
        .map_err(|e| e.to_string())?;
    Ok(new)
}

/// Reads one request line from `input`, serves it, writes one reply line to `output` (the `__fetch` command).
pub fn run(
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
    place: &Place,
) -> std::io::Result<()> {
    let mut line = String::new();
    input.read_line(&mut line)?;
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) => serve(&req, place),
        Err(e) => Reply::failed(format!("not a fetch request: {e}")),
    };
    let text = serde_json::to_string(&reply).map_err(std::io::Error::other)?;
    writeln!(output, "{text}")?;
    output.flush()
}

/// The longest a child may take: the download budget, and a little to start and unpack.
pub const CHILD_LIMIT: Duration = Duration::from_secs(mirror::DOWNLOAD_BUDGET.as_secs() + 60);

/// The daemon's side: each request runs in `program args… __fetch`, a child of its own.
#[derive(Debug, Clone)]
pub struct ChildDownloads {
    /// The doze executable (and any arguments before `__fetch`).
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Added to the child's environment (`DOZE_HOME`, a registry or mirror override).
    pub env: Vec<(String, String)>,
}

impl ChildDownloads {
    /// Asks a child to do `req`; its reply.
    pub async fn ask(&self, req: &Request) -> Result<Reply, Error> {
        let line = serde_json::to_string(req)
            .map_err(|e| Error::new(ErrorKind::Internal, e.to_string()))?;
        let mut child = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .arg("__fetch")
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::new(ErrorKind::Unavailable, format!("starting the fetch: {e}")))?;
        let talk = async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(format!("{line}\n").as_bytes()).await?;
            }
            let mut out = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                stdout.read_to_string(&mut out).await?;
            }
            child.wait().await?;
            Ok::<_, std::io::Error>(out)
        };
        let out = tokio::time::timeout(CHILD_LIMIT, talk)
            .await
            .map_err(|_| Error::new(ErrorKind::TimedOut, "the download did not finish in time"))?
            .map_err(|e| Error::new(ErrorKind::Unavailable, format!("the fetch: {e}")))?;
        serde_json::from_str(out.trim()).map_err(|e| {
            Error::new(
                ErrorKind::Internal,
                format!("the fetch answered {out:?}: {e}"),
            )
        })
    }

    async fn ok(&self, req: &Request) -> Result<Reply, Error> {
        let r = self.ask(req).await?;
        match r.error {
            Some(e) => Err(Error::new(ErrorKind::Unavailable, e)),
            None => Ok(r),
        }
    }
}

impl Downloads for ChildDownloads {
    fn resolve_major<'a>(
        &'a self,
        engine: &'a str,
        major: &'a str,
    ) -> doze_module_host::services::BoxFuture<'a, Result<String, Error>> {
        Box::pin(async move {
            let req = Request::ResolveMajor {
                engine: engine.into(),
                major: major.into(),
            };
            Ok(self.ok(&req).await?.version)
        })
    }

    fn ensure<'a>(
        &'a self,
        req: &'a Ensure,
    ) -> doze_module_host::services::BoxFuture<'a, Result<Ensured, Error>> {
        Box::pin(async move {
            let ask = Request::Ensure {
                engine: req.engine.clone(),
                full: req.full.clone(),
                target: req.target.clone(),
                expected_sha: req.expected_sha.clone(),
            };
            let r = self.ok(&ask).await?;
            Ok(Ensured {
                bin_dir: r.bin_dir,
                digest: r.digest,
            })
        })
    }
}

#[cfg(test)]
#[path = "child_tests.rs"]
mod tests;
