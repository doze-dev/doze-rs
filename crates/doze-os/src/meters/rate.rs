//! CPU as a share of a core over the time since the last reading of the same process: the change in its CPU time over
//! the change in the clock. What each process had at its last reading is kept here, for every caller (the daemon's
//! status and its stats topic read the same processes).

use super::{Proc, Raw};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Readings closer than this to the kept one give the kept share again: a share over a few milliseconds is noise.
const NARROWEST: Duration = Duration::from_millis(200);

/// A process not read for this long is forgotten (it ended, or nobody asks for it any more).
const FORGET: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy)]
struct Kept {
    at: Instant,
    cpu_ns: u64,
    share: f64,
}

static KEPT: Mutex<BTreeMap<u32, Kept>> = Mutex::new(BTreeMap::new());

fn kept() -> std::sync::MutexGuard<'static, BTreeMap<u32, Kept>> {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `pid` was read before, so its next reading has a share.
pub(super) fn known(pid: u32) -> bool {
    kept().contains_key(&pid)
}

/// `raws` read at `now`, each with its share of a core since its last reading: 0 for one read for the first time, or
/// whose CPU time went back (its pid was taken by a new process).
pub(super) fn rated(raws: Vec<Raw>, now: Instant) -> Vec<Proc> {
    let mut kept = kept();
    kept.retain(|_, k| now.saturating_duration_since(k.at) < FORGET);
    raws.into_iter()
        .map(|r| {
            let cpu = share(&mut kept, &r, now);
            Proc {
                pid: r.pid,
                ppid: r.ppid,
                rss: r.rss,
                cpu,
                cmd: r.cmd,
            }
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)] // WHY: nanoseconds of CPU and of the clock; a share is drawn, not summed exactly
fn share(kept: &mut BTreeMap<u32, Kept>, r: &Raw, now: Instant) -> f64 {
    if let Some(k) = kept.get(&r.pid).copied()
        && r.cpu_ns >= k.cpu_ns
    {
        let window = now.saturating_duration_since(k.at);
        if window < NARROWEST {
            return k.share;
        }
        let share = (r.cpu_ns - k.cpu_ns) as f64 / window.as_nanos() as f64 * 100.0;
        kept.insert(
            r.pid,
            Kept {
                at: now,
                cpu_ns: r.cpu_ns,
                share,
            },
        );
        return share;
    }
    kept.insert(
        r.pid,
        Kept {
            at: now,
            cpu_ns: r.cpu_ns,
            share: 0.0,
        },
    );
    0.0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;

    fn raw(pid: u32, cpu_ns: u64) -> Raw {
        Raw {
            pid,
            ppid: 1,
            rss: 0,
            cpu_ns,
            cmd: "x".into(),
        }
    }

    // Pids far above any real one, so the shared table holds nothing else under them.
    const A: u32 = u32::MAX - 10;
    const B: u32 = u32::MAX - 11;

    #[test]
    fn the_share_is_the_change_over_the_window_not_the_lifetime() {
        let t0 = Instant::now();
        // An hour of CPU already spent: a lifetime average would say a lot; the share since says what it does now.
        assert_eq!(rated(vec![raw(A, 3_600_000_000_000)], t0)[0].cpu, 0.0);
        assert!(known(A));
        let t1 = t0 + Duration::from_secs(1);
        let got = rated(vec![raw(A, 3_600_250_000_000)], t1)[0].cpu;
        assert!((got - 25.0).abs() < 1e-9, "{got}");
        // A reading a moment later gives the same share, not a share over 10 ms.
        let got = rated(
            vec![raw(A, 3_600_260_000_000)],
            t1 + Duration::from_millis(10),
        )[0]
        .cpu;
        assert!((got - 25.0).abs() < 1e-9, "{got}");
    }

    #[test]
    fn a_pid_taken_by_a_new_process_starts_again() {
        let t0 = Instant::now();
        let _ = rated(vec![raw(B, 5_000_000_000)], t0);
        let got = rated(vec![raw(B, 1_000)], t0 + Duration::from_secs(1))[0].cpu;
        assert_eq!(got, 0.0);
        let got = rated(vec![raw(B, 2_000_000_000)], t0 + Duration::from_secs(3))[0].cpu;
        assert!((got - 100.0).abs() < 1e-3, "{got}");
    }
}
