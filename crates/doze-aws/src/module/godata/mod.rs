//! Taking over the data a Go doze-aws module left in an instance's folder.
//!
//! Go kept each service's state in a bbolt file (`s3/meta.bolt`, `sqs/sqs.bolt`, …, JSON values) and S3's objects as
//! files under `s3/blobs/`; the Rust doze-aws keeps redb files of its own beside nothing it does not know. So the
//! take-over is two steps, both once:
//!
//! 1. **Set aside, at provision** (before the server first starts): every service folder holding a `.bolt` file moves
//!    whole under `go-data/` (renames, through `go-data.partial` so a stop half way is finished next time). The Rust
//!    server then starts on a folder with no Go files in it: none can be mistaken for its own or swept (Lambda's code
//!    sweep removes folders it does not know).
//! 2. **Replay, at converge** (after the declared stack is in place): S3 buckets and every object version, oldest first,
//!    and SQS queues and their messages, through the instance's own API, as a client would make them. The server's own
//!    rules hold for everything it then keeps. Each bucket and queue done is written to `go-data/.progress`, so a start
//!    stopped half way does not replay it twice; `go-data/.imported` says it is finished.
//!
//! Go's files stay in `go-data/` until a person removes them. What is not replayed is said in the instance's log and
//! named in `.imported`: services other than S3 and SQS that hold data, multipart uploads in progress, and bucket
//! settings beyond versioning and tags. Object version ids, dates and message ids are made again; messages in flight
//! come back visible.

mod s3;
mod sqs;

use std::path::{Path, PathBuf};

use crate::client::Client;

/// Where Go's files are kept, in the instance's folder.
pub(crate) const KEPT: &str = "go-data";
const PARTIAL: &str = "go-data.partial";
const PROGRESS: &str = ".progress";
const IMPORTED: &str = ".imported";

/// Whether `dir` (a service folder) is Go's: it holds a bbolt file and no redb file of this server's (a folder with both
/// is left where it is, so nothing of this server's is ever moved).
fn is_go(dir: &Path) -> bool {
    let has = |ext: &str| {
        std::fs::read_dir(dir).is_ok_and(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == ext))
        })
    };
    has("bolt") && !has("redb")
}

/// Step 1: moves Go's service folders under `go-data/`. `true` when it moved any.
///
/// # Errors
/// A rename failed; what was moved stays in `go-data.partial` and the next start goes on from there.
pub(crate) fn set_aside(data: &Path) -> Result<bool, String> {
    let kept = data.join(KEPT);
    let partial = data.join(PARTIAL);
    if kept.exists() {
        return Ok(false);
    }
    let go: Vec<PathBuf> = std::fs::read_dir(data)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && is_go(p))
        .collect();
    if go.is_empty() && !partial.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    for dir in go {
        let to = partial.join(dir.file_name().unwrap_or_default());
        std::fs::rename(&dir, &to).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::rename(&partial, &kept).map_err(|e| e.to_string())?;
    Ok(true)
}

/// What a replay did.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Replayed {
    pub(crate) buckets: usize,
    pub(crate) objects: usize,
    pub(crate) queues: usize,
    pub(crate) messages: usize,
    /// What was not carried, one line each.
    pub(crate) left: Vec<String>,
}

/// The names already done, from `.progress`.
fn done(kept: &Path) -> Vec<String> {
    std::fs::read_to_string(kept.join(PROGRESS))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Notes `what` (`s3:<bucket>`, `sqs:<queue>`) as done.
fn mark(kept: &Path, what: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(kept.join(PROGRESS))
        .map_err(|e| e.to_string())?;
    writeln!(f, "{what}").map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}

/// Step 2: replays what is in `go-data/` through the server at `client`; `None` when there is nothing (left) to do.
///
/// # Errors
/// A Go file cannot be read or the server refused a call; what was done is in `.progress`, and the next start goes on.
pub(crate) fn replay(data: &Path, client: &Client) -> Result<Option<Replayed>, String> {
    let kept = data.join(KEPT);
    if !kept.is_dir() || kept.join(IMPORTED).exists() {
        return Ok(None);
    }
    let finished = done(&kept);
    let mut r = Replayed::default();
    s3::replay(&kept.join("s3"), client, &finished, &mut r, &|w| {
        mark(&kept, w)
    })?;
    sqs::replay(&kept.join("sqs"), client, &finished, &mut r, &|w| {
        mark(&kept, w)
    })?;
    for e in std::fs::read_dir(&kept)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "s3" || name == "sqs" || !e.path().is_dir() {
            continue;
        }
        for f in std::fs::read_dir(e.path())
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if f.path().extension().is_some_and(|x| x == "bolt")
                && doze_bolt::Db::open(&f.path()).map_or(true, |db| db.holds_data().unwrap_or(true))
            {
                r.left.push(format!(
                    "{name}: its data was not carried (kept in {KEPT}/{name})"
                ));
            }
        }
    }
    let mut note = format!(
        "taken over from the Go doze-aws: {} buckets, {} objects, {} queues, {} messages\n",
        r.buckets, r.objects, r.queues, r.messages
    );
    for l in &r.left {
        note.push_str(&format!("not carried: {l}\n"));
    }
    std::fs::write(kept.join(IMPORTED), note).map_err(|e| e.to_string())?;
    Ok(Some(r))
}

/// The value of the first `<tag>…</tag>` in `xml` after `from`, and where it ends.
pub(super) fn xml_text<'a>(xml: &'a str, tag: &str, from: usize) -> Option<(&'a str, usize)> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.get(from..)?.find(&open)? + from + open.len();
    let end = xml.get(start..)?.find(&close)? + start;
    Some((xml.get(start..end)?, end + close.len()))
}

/// Every `<tag>…</tag>` value in `xml`.
pub(super) fn xml_all<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((v, next)) = xml_text(xml, tag, at) {
        out.push(v);
        at = next;
    }
    out
}

/// `s` as a URI path segment or query value (`/` kept when `slash`).
pub(super) fn enc(s: &str, slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    aws_core::uri::encode_into(&mut out, s.as_bytes(), slash);
    out
}

#[cfg(test)]
mod tests;
