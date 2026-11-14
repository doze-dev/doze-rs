//! The hover titles of a stack page's controls, in Go's words (`pages/overview.tsx`: `Switch`, the head's buttons).
//! GPUI draws them as tooltips; Go's `aria-label`s have no counterpart (GPUI has no accessibility tree).

use super::Row;

/// The paused lock (`.lock`).
pub const LOCK: &str = "enabled = false in doze.hcl: only the file brings it back";

/// The head's Activity button.
pub const ACTIVITY: &str =
    "How long each service slept, what woke it, what crashed: the last hour, day or week (A)";

/// The head's All logs button.
pub const ALL_LOGS: &str = "Every service's logs, merged (L)";

/// The head's Up switch while doze.hcl does not load.
pub const BROKEN_UP: &str = "doze.hcl does not load, so the stack cannot start";

/// A row's switch: on a stopped stack, what turning it on starts; else what flipping it does, or why it cannot.
#[must_use]
pub fn switch(r: &Row) -> String {
    if r.down {
        return if r.off {
            "paused in doze.hcl (enabled = false)".into()
        } else {
            format!("Start the stack with {} and what it needs", r.name)
        };
    }
    if r.frozen {
        "The stack's daemon is not answering; nothing can change until it does".into()
    } else {
        flip(r.on, r.off).into()
    }
}

/// What flipping a running service's switch does (the instance head's switch, and a row's): Go's `title`.
#[must_use]
pub fn flip(on: bool, off: bool) -> &'static str {
    if off {
        "Turned off: switch it on to bring it back and wake it"
    } else if on {
        "Switch off to put it to sleep"
    } else {
        "Switch on to wake it"
    }
}

#[cfg(test)]
mod tests {
    use super::{Row, flip, switch};

    #[test]
    fn a_switch_says_what_go_says() {
        let mut r = Row {
            name: "api".into(),
            ..Row::default()
        };
        assert_eq!(switch(&r), "Switch on to wake it");
        r.on = true;
        assert_eq!(switch(&r), "Switch off to put it to sleep");
        r.off = true;
        assert_eq!(
            switch(&r),
            "Turned off: switch it on to bring it back and wake it"
        );
        r.frozen = true;
        assert_eq!(
            switch(&r),
            "The stack's daemon is not answering; nothing can change until it does"
        );
        r.down = true;
        assert_eq!(switch(&r), "paused in doze.hcl (enabled = false)");
        r.off = false;
        assert_eq!(switch(&r), "Start the stack with api and what it needs");
    }

    #[test]
    fn the_instance_switch_says_what_go_says() {
        assert_eq!(flip(false, false), "Switch on to wake it");
        assert_eq!(flip(true, false), "Switch off to put it to sleep");
        assert_eq!(
            flip(true, true),
            "Turned off: switch it on to bring it back and wake it"
        );
    }
}
