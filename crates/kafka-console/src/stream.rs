//! The rules the console's two tails share (`GET /api/v1/tail/live` and `/api/v1/tail/messages`, CON-809,
//! `docs/port/console/live-design.md`).
//!
//! A person has asked for a tail (and for how fast: `pace=1|3|10` seconds, never under one, never over thirty); no other page
//! opens a stream. The server COALESCES: it looks once a pace and sends an event only when rows were added (or a folded row
//! grew), so the traffic rate of the cluster never becomes the update rate of the page. A comment keeps the connection alive
//! every 15 s. Every stream ends when the server's cancellation token does, so a graceful shutdown never waits on one.

use std::time::Duration;

/// The least time between two looks: the most events a second a tail can send, whatever it asks for.
pub const LEAST_PACE: u64 = 1;
/// The most a person can slow a tail to, in seconds (Realtime 1, Relaxed 3, Slow 10).
pub const MOST_PACE: u64 = 30;

/// The time between looks that a request for `pace` seconds is given: never faster than once a second, never slower than 30 s.
#[must_use]
pub fn pace_of(pace: Option<&str>) -> Duration {
    Duration::from_secs(
        pace.and_then(|p| p.parse::<u64>().ok())
            .unwrap_or(LEAST_PACE)
            .clamp(LEAST_PACE, MOST_PACE),
    )
}
/// The comment line sent every 15 s.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// The most records a Messages tail looks at in one look.
pub const MESSAGES_WINDOW: usize = 50;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pace_is_what_was_asked_within_the_caps() {
        assert_eq!(pace_of(None), Duration::from_secs(1));
        assert_eq!(pace_of(Some("3")), Duration::from_secs(3));
        assert_eq!(
            pace_of(Some("0")),
            Duration::from_secs(1),
            "never faster than once a second"
        );
        assert_eq!(pace_of(Some("9999")), Duration::from_secs(30));
        assert_eq!(pace_of(Some("-4")), Duration::from_secs(1));
        assert_eq!(pace_of(Some("fast")), Duration::from_secs(1));
    }
}
