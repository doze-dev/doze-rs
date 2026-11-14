//! What each running instance costs (Go's `daemon/meters.go` and the meters of `Status`): its resident memory, the CPU
//! of its process tree, its connections, and for an app, service or task the processes under it. Read through
//! [`doze_os::meters`]: one `ps` for every instance.
//!
//! Recent readings are kept (Go's ring: 240 frames, readings closer than 800 ms are one), so a screen that subscribes
//! starts with the last minutes instead of an empty chart. With no subscriber the daemon reads every 5 s, and only
//! while something runs; a subscriber's own readings fill the ring at its rate.

use crate::Daemon;
use doze_control::{Frame, InstanceView, ProcView, StatView, Timestamp};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

const KEPT: usize = 240;
const UNWATCHED_GAP: Duration = Duration::from_secs(5);
const DEDUPE: Duration = Duration::from_millis(800);

/// The kept readings and how many subscribers read their own.
#[derive(Debug, Default)]
pub(crate) struct Ring {
    frames: Mutex<VecDeque<(SystemTime, Frame)>>,
    watchers: AtomicUsize,
}

impl Ring {
    /// Keeps `f` (without the processes under each app), unless a reading was kept a moment before it.
    pub(crate) fn record(&self, at: SystemTime, f: &Frame) {
        if f.stats.is_empty() {
            return;
        }
        let mut frames = crate::lock(&self.frames);
        if let Some((last, _)) = frames.back()
            && at.duration_since(*last).unwrap_or_default() < DEDUPE
        {
            return;
        }
        let mut kept = f.clone();
        for s in &mut kept.stats {
            s.children.clear();
        }
        frames.push_back((at, kept));
        while frames.len() > KEPT {
            frames.pop_front();
        }
    }

    /// The kept readings, oldest first.
    pub(crate) fn history(&self) -> Vec<Frame> {
        crate::lock(&self.frames)
            .iter()
            .map(|(_, f)| f.clone())
            .collect()
    }

    fn last_at(&self) -> Option<SystemTime> {
        crate::lock(&self.frames).back().map(|(at, _)| *at)
    }

    /// A subscriber reads its own while the returned guard lives.
    pub(crate) fn watch(&self) -> Watching<'_> {
        self.watchers.fetch_add(1, Ordering::SeqCst);
        Watching(self)
    }
}

/// See [`Ring::watch`].
#[derive(Debug)]
pub(crate) struct Watching<'a>(&'a Ring);

impl Drop for Watching<'_> {
    fn drop(&mut self) {
        self.0.watchers.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether an instance shows the processes under it: what doze runs from a command (an app, service or task; a
/// module's own processes are its business, as Go showed them only for apps).
fn shows_children(d: &Daemon, name: &str) -> bool {
    d.inner.labels.get(name).is_some_and(|l| !l.kind.is_empty())
}

fn proc_view(n: &doze_os::meters::Node) -> ProcView {
    ProcView {
        pid: i64::from(n.proc.pid),
        rss: i64::try_from(n.proc.rss).unwrap_or(i64::MAX),
        cpu: n.proc.cpu,
        cmd: n.proc.cmd.clone(),
        depth: i64::from(n.depth),
    }
}

/// The meters of every running instance now, as a `stats` frame stamped with the time it was read.
pub(crate) async fn sample(d: &Daemon) -> (SystemTime, Frame) {
    let running: Vec<_> = d
        .engine()
        .snapshot()
        .into_iter()
        .filter(|i| i.state.running() && i.pid != 0)
        .collect();
    let pids: Vec<u32> = running.iter().map(|i| i.pid).collect();
    let meters = doze_os::meters::sample(&pids).await;
    let at = SystemTime::now();
    let stats = running
        .into_iter()
        .map(|i| {
            let m = meters.get(&i.pid).cloned().unwrap_or_default();
            let children = if shows_children(d, &i.name) {
                m.children.iter().map(proc_view).collect()
            } else {
                Vec::new()
            };
            StatView {
                name: i.name,
                pid: i64::from(i.pid),
                ram: i64::try_from(m.rss).unwrap_or(i64::MAX),
                cpu: m.cpu,
                conns: i64::from(i.conns),
                children,
            }
        })
        .collect();
    let frame = Frame {
        topic: "stats".into(),
        at: Timestamp::from_system_time(at),
        stats,
        ..Frame::default()
    };
    (at, frame)
}

/// Fills each running view's `ram`, `cpu` and (an app's) `children` (Go's `Status`).
pub(crate) async fn fill(d: &Daemon, views: &mut [InstanceView]) {
    let pids: Vec<u32> = views
        .iter()
        .filter_map(|v| u32::try_from(v.pid).ok().filter(|p| *p != 0))
        .collect();
    let meters = doze_os::meters::sample(&pids).await;
    for v in views {
        let Some(m) = u32::try_from(v.pid).ok().and_then(|p| meters.get(&p)) else {
            continue;
        };
        v.ram = i64::try_from(m.rss).unwrap_or(i64::MAX);
        v.cpu = m.cpu;
        if shows_children(d, &v.name) {
            v.children = m.children.iter().map(proc_view).collect();
        }
    }
}

/// Go's `runMeters`: every 5 s while nobody subscribes and something runs, a reading for the ring.
pub(crate) async fn keep(d: Daemon) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let ring = &d.inner.meters;
        let recent = ring
            .last_at()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|ago| ago < UNWATCHED_GAP);
        if ring.watchers.load(Ordering::SeqCst) > 0 || recent {
            continue;
        }
        if d.engine().snapshot().iter().any(|i| i.pid != 0) {
            let (at, f) = sample(&d).await;
            ring.record(at, &f);
        }
    }
}
