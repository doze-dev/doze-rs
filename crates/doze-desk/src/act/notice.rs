//! What the app says (Go `serve.Say`, `components/toasts.tsx`, `pages/recent.tsx`): each line is a toast for a moment
//! and is kept among the recent ones. At most five toasts show; a success goes after 4 s, a failure stays 12 s or
//! until dismissed. The last twenty lines are kept, newest first, for the Recent popover. A line that something else
//! already raises (a failed start, which its page shows) is only kept, with no toast.

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime};

/// How many lines are kept.
pub const NOTICES_KEPT: usize = 20;
/// How many toasts show at once.
pub const TOASTS_SHOWN: usize = 5;
/// How long a success shows.
pub const GOOD_FOR: Duration = Duration::from_secs(4);
/// How long a failure shows.
pub const BAD_FOR: Duration = Duration::from_secs(12);

/// One thing said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub at: SystemTime,
    pub text: String,
    pub bad: bool,
}

/// A notice on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub notice: Notice,
    pub shown: Instant,
}

impl Toast {
    /// When it goes.
    pub fn until(&self) -> Instant {
        self.shown + if self.notice.bad { BAD_FOR } else { GOOD_FOR }
    }
}

/// The lines said, and the toasts showing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notices {
    kept: VecDeque<Notice>,
    toasts: Vec<Toast>,
    next: u64,
}

impl Notices {
    /// Says `text`: a toast now, and a line kept. Returns the toast's id, so a toast that says something is under way
    /// can be taken away when it is over.
    pub fn say(&mut self, text: impl Into<String>, bad: bool, at: SystemTime, now: Instant) -> u64 {
        let notice = self.keep(text.into(), bad, at);
        self.next += 1;
        self.toasts.push(Toast {
            id: self.next,
            notice,
            shown: now,
        });
        if self.toasts.len() > TOASTS_SHOWN {
            let over = self.toasts.len() - TOASTS_SHOWN;
            self.toasts.drain(..over);
        }
        self.next
    }

    /// Keeps a failure among the recent lines with no toast: for what a page already shows.
    pub fn remember(&mut self, text: impl Into<String>, at: SystemTime) {
        self.keep(text.into(), true, at);
    }

    fn keep(&mut self, text: String, bad: bool, at: SystemTime) -> Notice {
        let n = Notice { at, text, bad };
        self.kept.push_back(n.clone());
        while self.kept.len() > NOTICES_KEPT {
            self.kept.pop_front();
        }
        n
    }

    /// The toasts showing, oldest first (the stack's top).
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// The lines kept, newest first.
    pub fn recent(&self) -> impl Iterator<Item = &Notice> {
        self.kept.iter().rev()
    }

    /// Takes away the toasts whose time is up; says whether any went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.toasts.len();
        self.toasts.retain(|t| t.until() > now);
        self.toasts.len() != before
    }

    /// How long until the next toast goes, if one shows.
    pub fn next_expiry(&self, now: Instant) -> Option<Duration> {
        self.toasts
            .iter()
            .map(|t| t.until().saturating_duration_since(now))
            .min()
    }

    /// Takes a toast away (its ×).
    pub fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::{BAD_FOR, GOOD_FOR, NOTICES_KEPT, Notices, TOASTS_SHOWN};
    use std::time::{Duration, Instant, SystemTime};

    #[test]
    fn five_toasts_at_most_and_twenty_lines_kept_newest_first() {
        let (mut n, now, at) = (Notices::default(), Instant::now(), SystemTime::now());
        for i in 0..25 {
            n.say(format!("line {i}"), false, at, now);
        }
        assert_eq!(n.toasts().len(), TOASTS_SHOWN);
        assert_eq!(
            n.toasts().first().map(|t| t.notice.text.as_str()),
            Some("line 20")
        );
        let recent: Vec<&str> = n.recent().map(|x| x.text.as_str()).collect();
        assert_eq!(recent.len(), NOTICES_KEPT);
        assert_eq!(recent.first(), Some(&"line 24"));
    }

    #[test]
    fn a_success_goes_after_4s_a_failure_after_12s() {
        let (mut n, now, at) = (Notices::default(), Instant::now(), SystemTime::now());
        n.say("copied", false, at, now);
        n.say("wake api failed: boom", true, at, now);
        assert_eq!(n.next_expiry(now), Some(GOOD_FOR));
        assert!(!n.expire(now + Duration::from_secs(3)));
        assert!(n.expire(now + GOOD_FOR + Duration::from_millis(1)));
        assert_eq!(n.toasts().len(), 1);
        assert!(n.expire(now + BAD_FOR + Duration::from_millis(1)));
        assert!(n.toasts().is_empty());
        assert_eq!(n.recent().count(), 2, "the lines stay kept");
    }

    #[test]
    fn remembered_lines_have_no_toast_and_a_dismissed_toast_goes() {
        let (mut n, now, at) = (Notices::default(), Instant::now(), SystemTime::now());
        n.remember("Couldn't start acme: boom", at);
        assert!(n.toasts().is_empty());
        assert!(n.recent().all(|x| x.bad));
        n.say("x", true, at, now);
        let id = n.toasts().first().map(|t| t.id).unwrap_or_default();
        n.dismiss(id);
        assert!(n.toasts().is_empty());
    }
}
