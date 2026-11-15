//! The connection to one daemon, as a page reports it: connecting, live, or down and when it retries. Folded from
//! what the follower says, as the Go hub's `keep` loop set it.

use doze_control::FollowEvent;
use std::time::SystemTime;

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
            }
            FollowEvent::Down { error, retry_in } => {
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
    }

    /// Whether a verb can be sent now.
    pub fn is_live(&self) -> bool {
        self.conn == Conn::Live
    }
}
