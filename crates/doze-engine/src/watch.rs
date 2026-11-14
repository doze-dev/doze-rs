//! Noticing that files changed, for `watch` on an app and for the inputs of the tasks it waits for (Go's
//! `internal/watch`). It polls, as Go chose: once an interval it lists what the globs match and compares each file's
//! size and modification time with the last look. A kernel notification holds a descriptor per watched file on macOS,
//! which a large repository runs out of; a poll costs a stat per matched file, once a second, only while the app is
//! awake. A change is reported once the files have been quiet for an interval, so a save that writes twice, or a
//! checkout that touches a hundred files, is one change.

use crate::tasks::inputs;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// How often the files are looked at.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(1);

/// One folder and the globs, relative to it, that matter in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    pub dir: PathBuf,
    pub globs: Vec<String>,
}

/// Every matched file and its size and modification time.
type Snapshot = BTreeMap<PathBuf, (u64, Option<SystemTime>)>;

fn snapshot(sets: &[Set]) -> Snapshot {
    let mut out = Snapshot::new();
    for s in sets {
        for rel in inputs::files(&s.dir, &s.globs) {
            let p = s.dir.join(&rel);
            if let Ok(m) = std::fs::metadata(&p) {
                out.insert(p, (m.len(), m.modified().ok()));
            }
        }
    }
    out
}

/// The paths that differ between two looks, sorted: changed, added or gone.
fn changed(prev: &Snapshot, cur: &Snapshot) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = cur
        .iter()
        .filter(|(p, sig)| prev.get(*p) != Some(sig))
        .map(|(p, _)| p.clone())
        .collect();
    out.extend(prev.keys().filter(|p| !cur.contains_key(*p)).cloned());
    out.sort();
    out
}

/// Calls `on_change` with the first changed path each time the files have changed and then been quiet for an interval,
/// until the future is dropped. `on_change` is awaited before the next look, and what it wrote is not a change.
pub async fn poll<F, Fut>(sets: Vec<Set>, interval: Duration, mut on_change: F)
where
    F: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let interval = if interval.is_zero() {
        DEFAULT_INTERVAL
    } else {
        interval
    };
    let look = |sets: Vec<Set>| async move {
        tokio::task::spawn_blocking(move || {
            let s = snapshot(&sets);
            (sets, s)
        })
        .await
        .ok()
    };
    let Some((mut sets, mut prev)) = look(sets).await else {
        return;
    };
    let mut pending: Option<PathBuf> = None;
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let Some((s, cur)) = look(sets).await else {
            return;
        };
        sets = s;
        let diff = changed(&prev, &cur);
        prev = cur;
        if let Some(first) = diff.into_iter().next() {
            // Still changing: wait for quiet, keeping the first path seen.
            pending.get_or_insert(first);
        } else if let Some(path) = pending.take() {
            on_change(path).await;
            let Some((s, after)) = look(sets).await else {
                return;
            };
            sets = s;
            prev = after;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use std::sync::{Arc, Mutex};

    fn write(p: &std::path::Path, body: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// Go's `TestAChangeIsReportedOnceAfterItSettles`: a file outside the globs is not a change; a burst is one change;
    /// a removal is a change.
    #[tokio::test]
    async fn a_change_is_reported_once_after_it_settles() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().to_path_buf();
        write(&dir.join("main.go"), "package main");
        write(&dir.join("notes.txt"), "not watched");
        let got = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
        let seen = got.clone();
        let sets = vec![Set {
            dir: dir.clone(),
            globs: vec!["**/*.go".into()],
        }];
        let task = tokio::spawn(poll(sets, Duration::from_millis(100), move |p| {
            seen.lock().unwrap().push(p);
            async {}
        }));
        let count = || got.lock().unwrap().len();
        tokio::time::sleep(Duration::from_millis(250)).await;
        write(&dir.join("notes.txt"), "edited, but not matched");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(count(), 0, "a file outside the globs was reported");
        for i in 0..5 {
            write(&dir.join("main.go"), &format!("package main // {i}"));
            write(&dir.join("sub/x.go"), "package sub");
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(
            count(),
            1,
            "one burst, one change: {:?}",
            got.lock().unwrap()
        );
        let first = got.lock().unwrap()[0].clone();
        assert!(
            first.ends_with("main.go") || first.ends_with("sub/x.go"),
            "{first:?}"
        );
        std::fs::remove_file(dir.join("main.go")).unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(count(), 2, "a removal is a change");
        task.abort();
    }
}
