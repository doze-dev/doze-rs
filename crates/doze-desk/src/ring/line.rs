//! One line of the ring: the fixed record it is held as, and the borrowed view a page reads.

use doze_control::{LogFrame, Timestamp};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One line as the ring holds it: 32 bytes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Meta {
    /// Nanoseconds since the epoch; `i64::MIN` when the daemon sent none.
    pub(super) nanos: i64,
    /// The arena, by its number.
    pub(super) arena: u32,
    pub(super) start: u32,
    pub(super) len: u32,
    pub(super) run: u32,
    pub(super) instance: u32,
    pub(super) stream: u32,
}

/// One line, borrowed from the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRef<'a> {
    pub instance: &'a str,
    pub line: &'a str,
    /// `o`, `e`, `o:<part>` or `s:<step>`.
    pub stream: &'a str,
    pub run: u32,
    /// When it was printed; `None` when the daemon did not say.
    pub time: Option<SystemTime>,
}

impl LineRef<'_> {
    /// The line as the daemon sent it.
    pub fn to_frame(&self) -> LogFrame {
        LogFrame {
            instance: self.instance.to_owned(),
            line: self.line.to_owned(),
            stream: self.stream.to_owned(),
            run: i64::from(self.run),
            time: self
                .time
                .map_or_else(Timestamp::zero, Timestamp::from_system_time),
        }
    }
}

pub(super) fn nanos_of(t: &Timestamp) -> i64 {
    if t.is_zero() {
        return i64::MIN;
    }
    t.unix_nanos()
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(i64::MIN)
}

pub(super) fn time_of(nanos: i64) -> Option<SystemTime> {
    if nanos == i64::MIN {
        return None;
    }
    let d = Duration::from_nanos(nanos.unsigned_abs());
    if nanos >= 0 {
        UNIX_EPOCH.checked_add(d)
    } else {
        UNIX_EPOCH.checked_sub(d)
    }
}

#[cfg(test)]
mod tests {
    use super::Meta;

    #[test]
    fn a_line_record_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Meta>(), 32);
    }
}
