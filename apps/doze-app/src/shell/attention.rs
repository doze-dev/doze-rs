//! Whether the person is using the window: it is the key window and they touched it (a key, a click, the pointer, the
//! wheel) in the last [`FOR`]. Opening it, or bringing it to the front, counts as a touch.
//!
//! Why it matters: text that changes with time alone (a dozing row's "sleeps in 0:48", a crash's "next in 4s", "last
//! update 5s ago") is read again once a second, and each read that changes the words draws a frame. While frames keep
//! coming, even one a second, the GPU driver holds about 150 MB for the process (docs/port/weight.md, "the idle
//! window"), which put an open, untouched window at 225 MB against its 85 MB budget. So the once-a-second reads run
//! only while the window is in use; an untouched window changes only when the hub brings news (Go's cadence: its
//! pages moved with the daemon's pushes), and a touch reads everything again at once, so the person never acts on a
//! figure older than the moment they came back.

use std::cell::Cell;
use std::time::{Duration, Instant};

/// How long after the last touch the window still counts as in use.
pub(crate) const FOR: Duration = Duration::from_secs(30);

thread_local! {
    /// The last touch (the main thread's: the one window lives there).
    static LAST: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Whether the window is the key window.
    static KEY: Cell<bool> = const { Cell::new(false) };
    /// Whether the window is on screen.
    static VISIBLE: Cell<bool> = const { Cell::new(true) };
}

/// The person touched the window at `now`; whether that brought it back into use (it is key, and was untouched until
/// then), so what is shown is read again. A pointer over a window that is not key changes nothing.
pub(crate) fn touch(now: Instant) -> bool {
    let was = attended(now);
    LAST.with(|l| l.set(Some(now)));
    attended(now) && !was
}

/// How long until the window, untouched from now on, stops being in use; `None` when it is not in use.
pub(crate) fn lapses_in(now: Instant) -> Option<Duration> {
    if !attended(now) {
        return None;
    }
    LAST.with(Cell::get)
        .map(|t| (t + FOR).saturating_duration_since(now))
}

/// Whether the window is on screen (not minimised, not hidden behind others), as the platform last said.
pub(crate) fn set_visible(v: bool) {
    VISIBLE.with(|x| x.set(v));
}

/// Whether the hub should stream what only a person looking needs (the meters): on screen and in use.
pub(crate) fn wants_meters(now: Instant) -> bool {
    VISIBLE.with(Cell::get) && attended(now)
}

/// The window became key, or stopped being key; becoming key counts as a touch. Whether it was untouched until then.
pub(crate) fn key(is_key: bool, now: Instant) -> bool {
    let was = attended(now);
    KEY.with(|k| k.set(is_key));
    if is_key {
        LAST.with(|l| l.set(Some(now)));
    }
    is_key && !was
}

/// Whether the window is in use at `now`: key, and touched in the last [`FOR`].
pub(crate) fn attended(now: Instant) -> bool {
    KEY.with(Cell::get)
        && LAST
            .with(Cell::get)
            .is_some_and(|t| now.saturating_duration_since(t) < FOR)
}

/// Forgets the window (it closed).
pub(crate) fn reset() {
    LAST.with(|l| l.set(None));
    KEY.with(|k| k.set(false));
    VISIBLE.with(|v| v.set(true));
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{FOR, attended, key, reset, touch};

    #[test]
    fn the_window_is_in_use_while_key_and_touched_lately() {
        reset();
        let t = Instant::now();
        assert!(!attended(t), "a window nobody has touched");
        assert!(
            !touch(t),
            "a touch on a window that is not key brings nothing back"
        );
        assert!(!attended(t), "a touch alone does not make a window key");
        assert!(
            key(true, t),
            "becoming key is a touch, after a time untouched"
        );
        assert!(attended(t + FOR - Duration::from_millis(1)));
        assert!(!attended(t + FOR), "untouched for FOR");
        assert!(touch(t + FOR), "and a touch after that brings it back");
        assert!(attended(t + FOR));
        key(false, t + FOR);
        assert!(!attended(t + FOR), "a window that is not key is not in use");
        reset();
    }
}
