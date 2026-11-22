//! The connection to one daemon, as a page reports it: connecting, live, or down and when it retries. Folded from
//! what the follower says, as the Go hub's `keep` loop set it.
//!
//! A connection that has just dropped is not yet a daemon that stopped answering: on every `doze down` the socket
//! closes as the daemon exits, a moment before the stack is seen stopped, and on every `doze up` the first dial can
//! come before the daemon listens. So a loss counts ([`Link::lost`]) only once it has lasted [`LOST_AFTER`]; until then
//! the pages say "connecting" ([`Link::shown`]), and Needs you says nothing.

use doze_control::FollowEvent;
use std::time::{Duration, SystemTime};

/// How long a connection must stay down before the daemon counts as not answering. The follower's backoff (500 ms,
/// then 1 s, 2 s, …) has failed twice by 1.5 s, so 2 s is two failed retries and a little more: a daemon that
/// exits or starts is through it, a crashed or hung one is flagged at once after it.
pub const LOST_AFTER: Duration = Duration::from_secs(2);

/// What the hub knows about one daemon's connection (Go `hub.Conn`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Conn {
    /// A subscription is being opened; nothing shown is live yet.
    #[default]
    Connecting,
    /// Frames are arriving.
    Live,
    /// The daemon stopped answering; the hub retries.
    Down,
}

/// The connection and its times.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    pub conn: Conn,
    /// While down: when the next attempt is.
    pub retry_at: Option<SystemTime>,
    /// When the last frame arrived.
    pub last_frame: Option<SystemTime>,
    /// While down: why, in the client's words.
    pub error: String,
    /// While down: since when (the first failure since it was last live, or since an action of this app ended).
    pub down_since: Option<SystemTime>,
}

impl Link {
    /// Folds in what the follower said about its connection; frames only move `last_frame` (see [`Link::frame`]).
    /// Returns whether anything a page shows changed.
    pub fn on(&mut self, e: &FollowEvent, now: SystemTime) -> bool {
        let before = self.clone();
        match e {
            // A stack that went down stays down, with its retry time, until it is live again: the page says
            // "retrying in 3s", not "connecting" on every attempt (as Go's hub did).
            FollowEvent::Connecting { .. } => {
                if self.conn != Conn::Down {
                    self.conn = Conn::Connecting;
                }
            }
            FollowEvent::Live => {
                self.conn = Conn::Live;
                self.retry_at = None;
                self.error.clear();
                self.last_frame = Some(now);
                self.down_since = None;
            }
            FollowEvent::Down { error, retry_in } => {
                if self.conn != Conn::Down || self.down_since.is_none() {
                    self.down_since = Some(now);
                }
                self.conn = Conn::Down;
                self.retry_at = now.checked_add(*retry_in);
                error.clone_into(&mut self.error);
            }
            FollowEvent::Frame(_) => self.frame(now),
        }
        *self != before
    }

    /// A frame arrived: the connection is live.
    pub fn frame(&mut self, now: SystemTime) {
        self.conn = Conn::Live;
        self.retry_at = None;
        self.last_frame = Some(now);
        self.down_since = None;
    }

    /// Whether a verb can be sent now.
    pub fn is_live(&self) -> bool {
        self.conn == Conn::Live
    }

    /// The connection has been down for [`LOST_AFTER`] or longer: the daemon is not answering.
    pub fn lost(&self, now: SystemTime) -> bool {
        self.conn == Conn::Down
            && self
                .down_since
                .is_some_and(|t| now.duration_since(t).unwrap_or_default() >= LOST_AFTER)
    }

    /// The connection as a page says it: a drop not yet [`Link::lost`] is still connecting.
    pub fn shown(&self, now: SystemTime) -> Conn {
        if self.conn == Conn::Down && !self.lost(now) {
            Conn::Connecting
        } else {
            self.conn
        }
    }

    /// An action of this app on the stack ended: a drop seen during it was the action's own doing, so a loss is
    /// counted from now.
    pub fn settle(&mut self, now: SystemTime) {
        if self.conn == Conn::Down {
            self.down_since = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Conn, LOST_AFTER, Link};
    use doze_control::FollowEvent;
    use std::time::{Duration, SystemTime};

    fn down() -> FollowEvent {
        FollowEvent::Down {
            error: "connection refused".into(),
            retry_in: Duration::from_millis(500),
        }
    }

    #[test]
    fn a_drop_is_a_loss_only_once_it_lasts() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut l = Link::default();
        l.on(&FollowEvent::Live, t0);
        l.on(&down(), t0);
        assert!(!l.lost(t0));
        assert_eq!(
            l.shown(t0),
            Conn::Connecting,
            "a fresh drop reads connecting"
        );
        // Each retry that fails keeps the time it first went down.
        let t1 = t0 + Duration::from_millis(1_500);
        l.on(&FollowEvent::Connecting { attempt: 2 }, t1);
        l.on(&down(), t1);
        assert_eq!(l.down_since, Some(t0));
        assert!(!l.lost(t1));
        let t2 = t0 + LOST_AFTER;
        assert!(l.lost(t2));
        assert_eq!(l.shown(t2), Conn::Down);
        // Live again: nothing lost.
        l.on(&FollowEvent::Live, t2);
        assert!(!l.lost(t2 + LOST_AFTER) && l.down_since.is_none());
    }

    #[test]
    fn an_action_ending_counts_a_loss_from_then() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let mut l = Link::default();
        l.on(&down(), t0);
        let end = t0 + Duration::from_secs(5);
        assert!(l.lost(end));
        l.settle(end);
        assert!(!l.lost(end + Duration::from_millis(1_900)));
        assert!(l.lost(end + LOST_AFTER));
        // A live link has nothing to settle.
        let mut live = Link::default();
        live.on(&FollowEvent::Live, t0);
        live.settle(end);
        assert_eq!(live.down_since, None);
    }
}
