//! One task for every `watch` of an engine (see [`Hub`]).

use super::{DEFAULT_INTERVAL, QUIET_STRIDE, Set, Snapshot, Tracker, snapshot};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// The next look aligned to `epoch`: `t`, moved later to the first multiple of `gap` after `epoch`. The quiet stride is
/// a whole number of intervals, so a quiet look also falls on an instant a busy one would.
fn aligned(
    t: tokio::time::Instant,
    gap: Duration,
    epoch: tokio::time::Instant,
) -> tokio::time::Instant {
    let gap_ns = gap.as_nanos().max(1);
    let since = t.saturating_duration_since(epoch).as_nanos();
    let steps = since.div_ceil(gap_ns);
    u64::try_from(steps.saturating_mul(gap_ns)).map_or(t, |ns| epoch + Duration::from_nanos(ns))
}

/// What one watcher does with a settled change. It must not block: the hub's lock is held.
pub(super) type React = Box<dyn Fn(PathBuf) + Send>;

struct Sub {
    sets: Vec<Set>,
    track: Tracker,
    react: React,
}

#[derive(Default)]
struct HubState {
    next_id: u64,
    subs: BTreeMap<u64, Sub>,
    /// Whether the hub's task runs (it ends when the last watcher goes, and starts again with the next).
    running: bool,
}

/// Every watcher of one engine, looked at by one task: one wakeup, one blocking look and one stat per file a gap, however
/// many apps watch (larder's seven Go and Python apps all watch `platform/**/*.go`: seven pollers stat'ed it seven
/// times, each on a timer and a blocking thread of its own). Each watcher keeps its own settling and quiet stride; the
/// hub looks at the shortest gap any of them wants, on the engine's grid ([`aligned`]). With no watcher there is no task
/// and no timer.
pub(crate) struct Hub {
    state: std::sync::Mutex<HubState>,
    /// A watcher came (its baseline is taken at once) or went.
    poke: tokio::sync::Notify,
    epoch: tokio::time::Instant,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub").finish_non_exhaustive()
    }
}

impl Hub {
    pub(crate) fn new() -> std::sync::Arc<Hub> {
        std::sync::Arc::new(Hub {
            state: std::sync::Mutex::new(HubState::default()),
            poke: tokio::sync::Notify::new(),
            epoch: tokio::time::Instant::now(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Watches `sets` until the returned [`Watching`] drops; `react` gets each settled change's first path.
    pub(crate) fn watch(self: &std::sync::Arc<Hub>, sets: Vec<Set>, react: React) -> Watching {
        let id = {
            let mut st = self.lock();
            let id = st.next_id;
            st.next_id += 1;
            st.subs.insert(
                id,
                Sub {
                    sets,
                    track: Tracker::new(tokio::time::Instant::now()),
                    react,
                },
            );
            if !st.running {
                st.running = true;
                tokio::spawn(hub_loop(std::sync::Arc::downgrade(self)));
            }
            id
        };
        self.poke.notify_one();
        Watching {
            hub: std::sync::Arc::downgrade(self),
            id,
        }
    }

    /// Every distinct folder and glob any watcher wants, one glob each (a glob is walked from its own root, so splitting
    /// costs nothing, and a glob two apps share is walked once), or `None` (and the task is to end) with no watcher left.
    fn wanted(&self) -> Option<Vec<Set>> {
        let mut st = self.lock();
        if st.subs.is_empty() {
            st.running = false;
            return None;
        }
        let mut sets: Vec<Set> = Vec::new();
        for s in st.subs.values().flat_map(|s| &s.sets) {
            for g in &s.globs {
                let one = Set {
                    dir: s.dir.clone(),
                    globs: vec![g.clone()],
                };
                if !sets.contains(&one) {
                    sets.push(one);
                }
            }
        }
        Some(sets)
    }

    /// Hands each watcher its part of a look; the gap before the next look.
    fn take(&self, looked: &[(Set, Snapshot)], interval: Duration) -> Duration {
        let now = tokio::time::Instant::now();
        let mut st = self.lock();
        let mut gap = interval * QUIET_STRIDE;
        for sub in st.subs.values_mut() {
            let mut cur = Snapshot::new();
            for (set, snap) in looked {
                if sub
                    .sets
                    .iter()
                    .any(|s| s.dir == set.dir && s.globs.iter().any(|g| set.globs.contains(g)))
                {
                    cur.extend(snap.iter().map(|(p, sig)| (p.clone(), *sig)));
                }
            }
            if let Some(path) = sub.track.look(cur, now) {
                (sub.react)(path);
            }
            gap = gap.min(sub.track.gap(interval, now));
        }
        gap
    }
}

async fn hub_loop(hub: std::sync::Weak<Hub>) {
    let mut next: Option<tokio::time::Instant> = None;
    loop {
        let Some(h) = hub.upgrade() else {
            return;
        };
        if let Some(at) = next {
            tokio::select! {
                () = tokio::time::sleep_until(at) => {}
                () = h.poke.notified() => {}
            }
        }
        let Some(sets) = h.wanted() else {
            return;
        };
        let looked = tokio::task::spawn_blocking(move || {
            sets.into_iter()
                .map(|s| {
                    let snap = snapshot(std::slice::from_ref(&s));
                    (s, snap)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let gap = h.take(&looked, DEFAULT_INTERVAL);
        next = Some(aligned(tokio::time::Instant::now() + gap, gap, h.epoch));
    }
}

/// One watcher of a [`Hub`]; dropping it stops the watching.
pub(crate) struct Watching {
    hub: std::sync::Weak<Hub>,
    id: u64,
}

impl std::fmt::Debug for Watching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watching").field("id", &self.id).finish()
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        if let Some(h) = self.hub.upgrade() {
            h.lock().subs.remove(&self.id);
            h.poke.notify_one();
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

    fn watched() -> (tempfile::TempDir, PathBuf, Arc<Mutex<usize>>, Vec<Set>) {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("main.go");
        write(&file, "package main");
        let sets = vec![Set {
            dir: t.path().to_path_buf(),
            globs: vec!["*.go".into()],
        }];
        (t, file, Arc::new(Mutex::new(0)), sets)
    }

    /// Two watchers of the same files share one hub: one change reaches each once; with both gone the hub's task ends,
    /// and a new watcher starts it again.
    #[tokio::test(start_paused = true)]
    async fn one_hub_serves_every_watcher_and_ends_with_the_last() {
        let (_t, file, a, sets) = watched();
        let b = Arc::new(Mutex::new(0usize));
        let hub = Hub::new();
        let count = |n: &Arc<Mutex<usize>>| -> React {
            let n = n.clone();
            Box::new(move |_| *n.lock().unwrap() += 1)
        };
        let wa = hub.watch(sets.clone(), count(&a));
        let wb = hub.watch(sets.clone(), count(&b));
        // The baselines are taken at once, on the poke.
        tokio::time::sleep(DEFAULT_INTERVAL / 2).await;
        write(&file, "package main // edited");
        tokio::time::sleep(DEFAULT_INTERVAL * 4).await;
        assert_eq!((*a.lock().unwrap(), *b.lock().unwrap()), (1, 1));
        drop((wa, wb));
        tokio::time::sleep(DEFAULT_INTERVAL * 4).await;
        assert!(!hub.lock().running, "no watcher, no task");
        let wc = hub.watch(sets, count(&a));
        tokio::time::sleep(DEFAULT_INTERVAL / 2).await;
        write(&file, "package main // edited again");
        tokio::time::sleep(DEFAULT_INTERVAL * 4).await;
        assert_eq!(*a.lock().unwrap(), 2, "a new watcher started it again");
        drop(wc);
    }

    /// A glob two watchers share is looked at once: larder's apps all watch `platform/**/*.go` beside globs of their own.
    #[tokio::test]
    async fn a_shared_glob_is_walked_once() {
        let hub = Hub::new();
        let set = |globs: &[&str]| Set {
            dir: PathBuf::from("/nonexistent"),
            globs: globs.iter().map(|g| (*g).to_string()).collect(),
        };
        let _a = hub.watch(
            vec![set(&["a/**/*.go", "platform/**/*.go"])],
            Box::new(|_| {}),
        );
        let _b = hub.watch(
            vec![set(&["b/**/*.go", "platform/**/*.go"])],
            Box::new(|_| {}),
        );
        let globs: Vec<Vec<String>> = hub.wanted().unwrap().into_iter().map(|s| s.globs).collect();
        assert_eq!(
            globs,
            [["a/**/*.go"], ["platform/**/*.go"], ["b/**/*.go"]]
                .map(|g| g.map(String::from).to_vec())
        );
    }

    /// Looks fall on the engine's grid: on a multiple of their gap after the epoch, never earlier than asked, so two
    /// pollers (one quiet, one not) wake together on every quiet look.
    #[test]
    fn looks_are_aligned_to_the_engines_grid() {
        let epoch = tokio::time::Instant::now();
        let s = Duration::from_secs;
        let ms = Duration::from_millis;
        assert_eq!(aligned(epoch + ms(1300), s(1), epoch), epoch + s(2));
        assert_eq!(
            aligned(epoch + s(2), s(1), epoch),
            epoch + s(2),
            "on the grid stays"
        );
        assert_eq!(aligned(epoch + ms(3100), s(3), epoch), epoch + s(6));
        // Before the epoch (cannot happen, but must not panic or go back): the epoch itself.
        assert_eq!(aligned(epoch, s(3), epoch), epoch);
    }
}
