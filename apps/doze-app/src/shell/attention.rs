//! How the window is being looked at, which says how often what it shows may change ([`Mode`]):
//! - **in use**: on screen, the key window, and touched (a key, a click, the pointer, the wheel) in the last [`FOR`];
//!   opening it, or bringing it to the front, counts as a touch;
//! - **glanced at**: on screen, but not in use;
//! - **hidden**: minimised, covered by other windows, on another Space or on a sleeping display (the platform's
//!   occlusion state).
//!
//! Why it matters: while frames keep coming, even one a second, the GPU driver holds about 150 MB for the process
//! (docs/port/weight.md, "the idle window"). Text that changes with time alone (a dozing row's "sleeps in 0:48", a
//! crash's "next in 4s", "last update 5s ago") read again every second is a frame every second. So it is read every
//! second only in use; glanced at, it is written to the minute and read again only when its words would change
//! (`shell::clock`); hidden, nothing is drawn, and what changed is drawn when the window shows again.

use std::cell::Cell;
use std::time::{Duration, Instant};

/// How long after the last touch the window still counts as in use.
pub(crate) const FOR: Duration = Duration::from_secs(30);

/// How the window is being looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Not on screen: no frames of its own.
    Hidden,
    /// On screen, not in use: words to the minute, changes of state at most once a second, the meters every ten.
    Glance,
    /// In use: everything to the second, as it happens.
    Use,
}

thread_local! {
    /// The last touch (the main thread's: the one window lives there).
    static LAST: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Whether the window is the key window.
    static KEY: Cell<bool> = const { Cell::new(false) };
    /// Whether the window is on screen.
    static VISIBLE: Cell<bool> = const { Cell::new(true) };
}

/// How the window is being looked at, at `now`.
pub(crate) fn mode(now: Instant) -> Mode {
    if !visible() {
        Mode::Hidden
    } else if attended(now) {
        Mode::Use
    } else {
        Mode::Glance
    }
}

/// Whether the window is in use at `now`: on screen, key, and touched in the last [`FOR`].
pub(crate) fn in_use(now: Instant) -> bool {
    mode(now) == Mode::Use
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

/// Whether the window is on screen (not minimised, not covered, not on a sleeping display), as the platform last said.
pub(crate) fn set_visible(v: bool) {
    VISIBLE.with(|x| x.set(v));
}

/// Whether the window is on screen.
pub(crate) fn visible() -> bool {
    VISIBLE.with(Cell::get)
}

/// Whether the hub should stream the meters: while the window is on screen, in use or not (glanced at, the page
/// takes them in every ten seconds; and a page that hears none could not say its daemon went quiet).
pub(crate) fn wants_meters() -> bool {
    visible()
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

/// Whether the window is key and was touched in the last [`FOR`] at `now` (on screen or not).
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

    use super::{FOR, Mode, attended, key, mode, reset, set_visible, touch};

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

    #[test]
    fn a_window_off_screen_is_hidden_whatever_its_use() {
        reset();
        let t = Instant::now();
        assert_eq!(mode(t), Mode::Glance, "on screen, untouched");
        key(true, t);
        assert_eq!(mode(t), Mode::Use);
        set_visible(false);
        assert_eq!(mode(t), Mode::Hidden, "key and touched, but covered");
        set_visible(true);
        assert_eq!(mode(t + FOR), Mode::Glance, "left alone");
        reset();
    }
}
