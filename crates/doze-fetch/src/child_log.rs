//! What a `__fetch` child says on its log (the daemon's): the slow part as it starts and its end, and how whoever
//! waits on the daemon reads those lines.

use super::{Place, Request};
use crate::lock::Lock;
use crate::registry::source::Source;
use std::path::Path;

/// The log line that ends a download [`super::run`] said it started, with how long it took.
pub(super) const END_DONE: &str = "doze: download done in";
/// The same, for one that failed (why goes back to the daemon, which says it).
pub(super) const END_FAILED: &str = "doze: download failed after";

/// How a line of the daemon's log changes the downloads under way: `Some(true)` for one a `__fetch` child starts
/// ("doze: downloading …", "doze: fetching module …"), `Some(false)` for one it ends, `None` for any other line. A
/// daemon starting for a fresh doze home fetches its modules before it serves; whoever waits for it reads this.
pub fn busy(line: &str) -> Option<bool> {
    if line.starts_with("doze: downloading ") || line.starts_with("doze: fetching module ") {
        Some(true)
    } else if line.starts_with(END_DONE) || line.starts_with(END_FAILED) {
        Some(false)
    } else {
        None
    }
}

/// What `req` is about to do that takes a while, in a line for the log: an engine build or a download not yet on this
/// machine, or a module build that is not pinned and cached here (its index is fetched, and its build when new).
pub(super) fn starting(req: &Request, place: &Place) -> Option<String> {
    if let Request::Module { source, lock, .. } = req {
        let src = Source::parse(source)?;
        return (!module_cached(place, &src, lock.as_deref())).then(|| {
            let from = place
                .modules
                .registry
                .bases
                .first()
                .map_or_else(String::new, |b| format!(" from {}", b.name()));
            format!("fetching module {src}{from}…")
        });
    }
    if let Request::Download { url, .. } = req {
        let cached = archive_cached(&place.modules.home, url);
        return (!cached).then(|| format!("downloading {url}…"));
    }
    let Request::Ensure {
        engine,
        full,
        target,
        ..
    } = req
    else {
        return None;
    };
    (!place.mirror.has(engine, full, target)).then(|| {
        format!(
            "downloading {engine} {full} ({target}) from {}…",
            place.mirror.source(engine)
        )
    })
}

/// Whether `src` is pinned in the lock at `lock` and that build is in the cache: resolving it touches no network.
fn module_cached(place: &Place, src: &Source, lock: Option<&Path>) -> bool {
    let Some(Ok(l)) = lock.map(Lock::load) else {
        return false;
    };
    l.modules.get(&src.to_string()).is_some_and(|pin| {
        pin.protocol >= place.modules.protocol
            && crate::archive::has_files(&place.modules.content_dir(src, &pin.version).join("bin"))
    })
}

fn archive_cached(home: &Path, url: &str) -> bool {
    crate::archive::recorded_sha(&crate::download::content_dir(home, url)).is_some()
}
