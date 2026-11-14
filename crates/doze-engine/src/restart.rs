//! What happens after a supervised process exits by itself: its restart policy, the backoff, and the budget (Go's
//! `shouldRestart`, `backoffFor`, `restartStabilityWindow`).

use std::time::Duration;

/// When a supervised process is started again after it exits by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Never.
    No,
    /// After a non-zero exit (the process engine's default).
    #[default]
    OnFailure,
    /// After any exit.
    Always,
}

impl Policy {
    /// The doze.hcl word: `no`, `on_failure`, `always`.
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::No => "no",
            Policy::OnFailure => "on_failure",
            Policy::Always => "always",
        }
    }
}

/// A restart policy with its first delay and its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restart {
    pub policy: Policy,
    /// The first delay; it doubles per attempt up to [`MAX_BACKOFF`]. Zero means one second.
    pub backoff: Duration,
    /// Attempts allowed in a rapid crash loop; zero never restarts.
    pub max_retries: u32,
}

impl Default for Restart {
    /// The process engine's defaults: on failure, from one second, five attempts.
    fn default() -> Restart {
        Restart {
            policy: Policy::OnFailure,
            backoff: Duration::from_secs(1),
            max_retries: 5,
        }
    }
}

impl Restart {
    /// Never restarts.
    pub const NEVER: Restart = Restart {
        policy: Policy::No,
        backoff: Duration::ZERO,
        max_retries: 0,
    };

    /// Whether this policy restarts at all (Go: not `no`, and a budget above zero).
    pub fn active(&self) -> bool {
        self.policy != Policy::No && self.max_retries > 0
    }

    /// Whether an exit warrants a restart: `always` on any, `on_failure` on a non-zero one.
    pub fn wants(&self, success: bool) -> bool {
        match self.policy {
            Policy::Always => true,
            Policy::OnFailure => !success,
            Policy::No => false,
        }
    }
}

/// The longest wait between attempts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// How long a restarted process must stay up for a later crash to count as isolated: the budget then starts over. The
/// budget bounds rapid crash loops, not crashes over a long life.
pub const STABILITY_WINDOW: Duration = Duration::from_secs(60);

/// The delay before attempt `attempt` (1-based): `base` doubled per earlier attempt, capped at [`MAX_BACKOFF`].
pub fn backoff_for(base: Duration, attempt: u32) -> Duration {
    let mut d = if base.is_zero() {
        Duration::from_secs(1)
    } else {
        base
    };
    for _ in 1..attempt {
        d = d.saturating_mul(2);
        if d >= MAX_BACKOFF {
            return MAX_BACKOFF;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps_as_go_does() {
        let s = Duration::from_secs;
        assert_eq!(backoff_for(s(1), 1), s(1));
        assert_eq!(backoff_for(s(1), 2), s(2));
        assert_eq!(backoff_for(s(1), 3), s(4));
        assert_eq!(backoff_for(s(1), 6), MAX_BACKOFF);
        assert_eq!(backoff_for(Duration::ZERO, 1), s(1));
        assert_eq!(backoff_for(s(20), 2), MAX_BACKOFF);
        // A large base is returned as is for the first attempt (Go only caps while doubling).
        assert_eq!(backoff_for(s(45), 1), s(45));
        assert_eq!(backoff_for(s(1), u32::MAX), MAX_BACKOFF);
    }

    #[test]
    fn policies_decide_as_go_does() {
        let r = |policy| Restart {
            policy,
            ..Restart::default()
        };
        assert!(r(Policy::Always).wants(true) && r(Policy::Always).wants(false));
        assert!(!r(Policy::OnFailure).wants(true) && r(Policy::OnFailure).wants(false));
        assert!(!r(Policy::No).wants(false));
        assert!(!Restart::NEVER.active());
        assert!(
            !Restart {
                max_retries: 0,
                ..Restart::default()
            }
            .active()
        );
        assert_eq!(Policy::OnFailure.as_str(), "on_failure");
    }
}
