//! One instance's meters over time, bounded as the Go hub bounded them: four minutes at a reading a second.

use doze_control::StatView;
use std::time::SystemTime;

/// How many readings an instance keeps.
pub const SAMPLES_KEPT: usize = 240;

/// One instance's meters, oldest first (Go `hub.Series`). `at` has the time of each reading when the daemon stamped
/// it; empty when one was not stamped (then a second apart is assumed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Series {
    pub ram: Vec<f64>,
    pub cpu: Vec<f64>,
    pub conns: Vec<f64>,
    pub at: Vec<SystemTime>,
}

impl Series {
    /// Adds a reading taken at `at`. A reading already held is sent again on a reconnect; it is skipped. Returns
    /// whether it was added.
    #[allow(clippy::cast_precision_loss)] // WHY: bytes and connections drawn on a chart; f64 holds 2^53 exactly
    pub fn push(&mut self, v: &StatView, at: Option<SystemTime>) -> bool {
        if let (Some(at), Some(last)) = (at, self.at.last())
            && at <= *last
        {
            return false;
        }
        keep(&mut self.ram, v.ram as f64);
        keep(&mut self.cpu, v.cpu);
        keep(&mut self.conns, v.conns as f64);
        match at {
            Some(at) => keep(&mut self.at, at),
            None => self.at.clear(),
        }
        true
    }

    /// How many readings it holds.
    pub fn len(&self) -> usize {
        self.ram.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.ram.is_empty()
    }
}

fn keep<T>(vs: &mut Vec<T>, v: T) {
    vs.push(v);
    if vs.len() > SAMPLES_KEPT {
        let extra = vs.len() - SAMPLES_KEPT;
        vs.drain(..extra);
    }
}
