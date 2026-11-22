//! A live daemon's beat from the fixture: its meters every second (as Go's fixture server sent them), and for the
//! tests of a window left alone on a live stack (`shell::live_tests`), meters that move every second and the instances
//! sent again with nothing changed, as a real daemon does.

use doze_control::Frame;
use doze_control::fixture::{Script, Step};

impl super::Fixture {
    /// One second of a live stack (`n` counts them): meters that move every second (memory, CPU and connections, as
    /// a real daemon's do), and every third second the instances again with nothing changed, as a real daemon sends
    /// them. Whether the daemon sends meters at all (a `-stale` one does not).
    #[cfg(all(test, feature = "capture"))]
    pub(crate) fn live(&self, n: u64) -> bool {
        let (Some(beat), Some(d)) = (&self.beat, &self.daemon) else {
            return false;
        };
        let mut f = beat.clone();
        // No time on the frame: the series takes every one (a stamped frame older than the last is dropped).
        f.at = doze_control::Timestamp::default();
        let k = i64::try_from(n).unwrap_or(0);
        for (i, st) in f.stats.iter_mut().enumerate() {
            let i = i64::try_from(i).unwrap_or(0);
            st.ram += (((k + i) % 7) * 3) << 20;
            st.cpu = ((k + i) % 9) as f64 * 1.5;
            st.conns = (k + i) % 4;
        }
        d.push(&f);
        if n.is_multiple_of(3)
            && let Some(p) = &self.pulse
        {
            d.push(p);
        }
        true
    }
}

/// The script's last frame of instances, as an update (not a snapshot), for a live second to send again.
pub(super) fn last_instances(s: &Script) -> Option<Frame> {
    s.steps.iter().rev().find_map(|st| match st {
        Step::Frame(f) if !f.instances.is_empty() => {
            let mut f = (**f).clone();
            f.snapshot = false;
            Some(f)
        }
        _ => None,
    })
}

/// The script's last meters frame: what a daemon goes on sending.
pub(super) fn last_meters(s: &Script) -> Option<Frame> {
    s.steps.iter().rev().find_map(|st| match st {
        Step::Frame(f) if f.topic == doze_control::Topic::Stats.as_str() => Some((**f).clone()),
        _ => None,
    })
}

/// As Go's fixture server does: the meters every second after the script, as a daemon sends them, so a metered page is
/// never told its picture is old; until the daemon stops.
pub(super) fn tick(p: doze_control::fixture::Pusher, meters: Option<Frame>) {
    let Some(f) = meters else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("fixture-meters".into())
        .spawn(move || {
            while p.push(&f) {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
}
