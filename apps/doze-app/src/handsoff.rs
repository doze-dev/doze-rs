//! Hands off: a window opened by a test, a shot, the bench or the scripted driver never takes the person's input. It
//! runs on the owner's live screen, so a keystroke meant for another app must not reach it (a dry run once saw an
//! `up --last` and a Run press nobody scripted).
//!
//! Three walls, each enough on its own for the common case:
//! - the window opens without focus and the app never activates (menu-bar policy, no Dock icon, not in ⌘Tab), so the
//!   system sends the keyboard elsewhere;
//! - the window lets the pointer through to whatever is under it (`doze_os::ui::hands_off`);
//! - inside the app, every key and pointer event is dropped unless the driver sends it ([`sending`]).
//!
//! After the window opens, [`check`] asserts the first two held (the app is not active, no window is key) and stops
//! the run if they did not.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, DispatchPhase, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollEvent, Styled as _, Window,
};

thread_local! {
    /// Hands-off is on. Per thread: input, windows and the driver are all the main thread's, and a test thread keeps
    /// its own.
    static ON: Cell<bool> = const { Cell::new(false) };
    /// The driver is dispatching an event of its own.
    static SENDING: Cell<bool> = const { Cell::new(false) };
}

/// Turns hands-off on for this run (on the main thread, before any window opens). Also on when `DOZE_APP_HANDS_OFF` is set or a
/// script plays (`DOZE_APP_SCRIPT`).
pub(crate) fn set() {
    ON.with(|o| o.set(true));
}

/// Whether this run keeps its hands off the person's input.
pub(crate) fn on() -> bool {
    ON.with(Cell::get)
}

/// Turns hands-off on when the environment asks for it: `DOZE_APP_HANDS_OFF=1`, or a scripted run.
pub(crate) fn from_env() {
    let set_in = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if set_in("DOZE_APP_HANDS_OFF") || set_in("DOZE_APP_SCRIPT") {
        set();
    }
}

/// Runs `f` (the driver dispatching an event) with its events let through.
#[cfg(any(test, feature = "fixtures"))]
pub(crate) fn sending<R>(f: impl FnOnce() -> R) -> R {
    /// Puts the wall back even if `f` panics.
    struct Back(bool);
    impl Drop for Back {
        fn drop(&mut self) {
            SENDING.with(|s| s.set(self.0));
        }
    }
    let _back = Back(SENDING.with(|s| s.replace(true)));
    f()
}

/// Whether an input event arriving now is refused: hands-off is on and the driver did not send it.
pub(crate) fn refuses() -> bool {
    on() && !SENDING.with(Cell::get)
}

/// Drops every keystroke the driver did not send, before any binding or text field sees it. Once, at boot.
pub(crate) fn guard_keys(cx: &mut App) {
    if !on() {
        return;
    }
    cx.intercept_keystrokes(|ev, _, cx| {
        if refuses() {
            refused("key", &ev.keystroke.key);
            cx.stop_propagation();
        }
    })
    .detach();
}

/// The root's first child while hands-off is on: drops every pointer event the driver did not send, in the capture
/// phase, before any element sees it.
pub(crate) fn pointer_wall() -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        |_, _, window, _| {
            if !on() {
                return;
            }
            wall::<MouseDownEvent>(window, "mouse-down");
            wall::<MouseUpEvent>(window, "mouse-up");
            wall::<MouseMoveEvent>(window, "mouse-move");
            wall::<ScrollEvent>(window, "scroll");
        },
    )
    .absolute()
    .size_0()
}

fn wall<E: gpui::MouseEvent>(window: &mut Window, what: &'static str) {
    window.on_mouse_event(move |_: &E, phase, _, cx| {
        if phase == DispatchPhase::Capture && refuses() {
            refused(what, "");
            cx.stop_propagation();
        }
    });
}

fn refused(what: &str, detail: &str) {
    #[cfg(feature = "fixtures")]
    crate::driver::note("refused", &format!("{what} {detail}"));
    #[cfg(not(feature = "fixtures"))]
    let _ = (what, detail);
}

/// After a hands-off window is on screen: makes it let the pointer through, then asserts the app is not active and no
/// window is key. A run that took the person's focus stops at once (exit 3), before it can act on their input.
pub(crate) fn check(window: &mut Window) {
    if !on() {
        return;
    }
    let walled = doze_os::ui::hands_off();
    let (active, key) = doze_os::ui::holds_input();
    let line = format!(
        "hands_off windows_walled={walled} app_active={active} key_windows={key} window_active={}",
        window.is_window_active()
    );
    #[cfg(feature = "fixtures")]
    crate::driver::note("hands-off", &line);
    if active || key > 0 || window.is_window_active() {
        eprintln!("doze-app: a hands-off run took the focus ({line}); stopping");
        std::process::exit(3);
    }
}

/// [`check`] on a window just opened.
pub(crate) fn after_open(h: gpui::AnyWindowHandle, cx: &mut App) {
    if on() {
        let _ = cx.update_window(h, |_, window, _| check(window));
    }
}

/// [`check`] again on every window, as a scripted run goes (before each step).
#[cfg(feature = "fixtures")]
pub(crate) fn recheck(cx: &mut App) {
    if on() {
        for h in cx.windows() {
            after_open(h, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{on, refuses, sending, set};

    #[test]
    fn with_hands_off_only_what_the_driver_sends_gets_through() {
        set();
        assert!(on());
        assert!(refuses(), "a person's event is refused");
        assert!(!sending(refuses), "the driver's own event gets through");
        assert!(
            sending(|| sending(|| true) && !refuses()),
            "nested sends stay let through"
        );
        assert!(refuses(), "and the wall is back after the send");
    }
}
