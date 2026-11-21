//! How finely the words that change with time alone are written: a countdown ("sleeps in 0:48"), an age ("up 12s",
//! "last update 5s ago"). Fine is Go's, to the second, and the default everywhere. Coarse is to the minute ("sleeps in
//! 4m", "sleeps in <1m", "up <1m"), for a window someone can see but is not using: its words then change at most once
//! a minute each, so it draws a frame at most that often (each frame keeps the GPU driver's ~150 MB, see the app's
//! docs/port/weight.md, "the idle window").
//!
//! Coarse words also say when they will next read differently: [`coarse`] returns the soonest such moment among the
//! words written inside it, so a window can wake once, then, instead of reading its words again every second.

use std::cell::Cell;
use std::time::Duration;

use crate::gotime::{self, HOUR, MINUTE};

thread_local! {
    /// Words written on this thread are coarse.
    static COARSE: Cell<bool> = const { Cell::new(false) };
    /// The soonest a coarse word written inside [`coarse`] reads differently, in nanoseconds from when it was written.
    static NEXT: Cell<Option<i64>> = const { Cell::new(None) };
}

/// Runs `f` with the words it writes coarse, and says, with what it returns, how soon the first of them will read
/// differently (`None` when none of them changes with time). Nested calls fold into the outer one.
pub fn coarse<R>(f: impl FnOnce() -> R) -> (R, Option<Duration>) {
    /// Puts the outer state back even if `f` panics.
    struct Back(bool, Option<i64>);
    impl Drop for Back {
        fn drop(&mut self) {
            COARSE.with(|c| c.set(self.0));
            let inner = NEXT.with(Cell::get);
            NEXT.with(|n| n.set(sooner(self.1, inner).filter(|_| self.0)));
        }
    }
    let back = Back(
        COARSE.with(|c| c.replace(true)),
        NEXT.with(|n| n.replace(None)),
    );
    let out = f();
    let next = NEXT.with(Cell::get);
    drop(back);
    let next = next.map(|n| Duration::from_nanos(u64::try_from(n.max(0)).unwrap_or(0)));
    (out, next)
}

/// Whether the words written now are coarse (inside [`coarse`]).
#[must_use]
pub fn is_coarse() -> bool {
    COARSE.with(Cell::get)
}

fn sooner(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// A coarse word just written reads differently `in_nanos` from now.
fn changes_in(in_nanos: i64) {
    if is_coarse() {
        NEXT.with(|n| n.set(sooner(n.get(), Some(in_nanos.max(1)))));
    }
}

/// A countdown with `left` nanoseconds to go, to the minute: `4m`, or `<1m` in its last minute (the units of Go's
/// `Ago`). It reads differently as the next whole minute is crossed (at once when `left` is a whole number of
/// minutes: "4m" turns "3m" just after 4:00), and when it runs out.
pub(crate) fn countdown(left: i64) -> String {
    let left = left.max(0);
    let rest = left % MINUTE;
    changes_in(if rest == 0 { 1 } else { rest });
    if left < MINUTE {
        "<1m".to_owned()
    } else {
        format!("{}m", left / MINUTE)
    }
}

/// An age of `d` nanoseconds in Go's `Ago` units, but never in seconds: `<1m` under a minute (or when the clocks
/// disagree and it is negative). Returns `None` from a minute on, where Go's own words are already this coarse; it
/// notes when the age reads differently either way.
pub(crate) fn age(d: i64) -> Option<String> {
    if d < MINUTE {
        changes_in(MINUTE.saturating_sub(d));
        return Some("<1m".to_owned());
    }
    let unit = if d < HOUR {
        MINUTE
    } else if d < 48 * HOUR {
        HOUR
    } else {
        24 * HOUR
    };
    changes_in(unit - d % unit);
    None
}

/// How much of a countdown of `idle` a ring shows gone, `gone` of it gone and `left` to go: all of it when fine;
/// when coarse, to the next whole minute left, so the ring moves when the words do ("4m" reads while 4:00 to 4:59 is
/// left, and the ring stands at 5:00 left).
#[must_use]
pub fn ring_gone(gone: Duration, left: Duration, idle: Duration) -> Duration {
    if !is_coarse() {
        return gone;
    }
    let mins = left.as_secs() / 60 + 1;
    idle.saturating_sub(Duration::from_secs(mins * 60))
}

/// Seconds to go (`left` as it is, unrounded), as `4s` when fine (Go's words, to the nearest second), as [`countdown`]
/// when coarse. Its callers stop saying it once it rounds to no second (under half a second left), so the
/// coarse word counts to that moment too.
pub(crate) fn seconds_left(left: i64) -> String {
    if is_coarse() {
        countdown(left - gotime::SECOND / 2)
    } else {
        format!(
            "{}s",
            gotime::whole_seconds(gotime::round(left, gotime::SECOND))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gotime::SECOND;

    #[test]
    fn coarse_words_are_to_the_minute_and_say_when_they_change() {
        let (w, next) = coarse(|| countdown(4 * MINUTE + 30 * SECOND));
        assert_eq!(w, "4m");
        assert_eq!(next, Some(Duration::from_secs(30)));
        let (w, next) = coarse(|| countdown(4 * MINUTE));
        assert_eq!(
            (w.as_str(), next),
            ("4m", Some(Duration::from_nanos(1))),
            "3:59.9… next"
        );
        let (w, next) = coarse(|| countdown(48 * SECOND));
        assert_eq!((w.as_str(), next), ("<1m", Some(Duration::from_secs(48))));
        let (w, next) = coarse(|| age(12 * SECOND));
        assert_eq!(
            (w.as_deref(), next),
            (Some("<1m"), Some(Duration::from_secs(48)))
        );
        let (w, next) = coarse(|| age(3 * MINUTE + 10 * SECOND));
        assert_eq!((w, next), (None, Some(Duration::from_secs(50))));
        let (_, next) = coarse(|| age(2 * HOUR + 10 * MINUTE));
        assert_eq!(next, Some(Duration::from_secs(50 * 60)));
    }

    #[test]
    fn the_soonest_change_wins_and_fine_words_note_nothing() {
        let (_, next) = coarse(|| {
            countdown(5 * MINUTE + 20 * SECOND);
            age(59 * SECOND);
            countdown(3 * MINUTE + 40 * SECOND)
        });
        assert_eq!(next, Some(Duration::from_secs(1)));
        assert!(!is_coarse(), "coarse ends with its call");
        assert_eq!(seconds_left(4 * SECOND), "4s", "fine outside");
        let ((), next) = coarse(|| {
            let ((), inner) = coarse(|| {
                countdown(90 * SECOND);
            });
            assert_eq!(inner, Some(Duration::from_secs(30)));
        });
        assert_eq!(
            next,
            Some(Duration::from_secs(30)),
            "folded into the outer call"
        );
    }

    #[test]
    fn the_surfaces_words_go_to_the_minute_only_inside_coarse() {
        let quiet = Duration::from_secs(5);
        let fine = crate::banner(false, false, Duration::ZERO, quiet);
        assert_eq!(fine.as_deref(), Some("last update 5s ago"), "Go's words");
        let (coarse_said, next) = coarse(|| crate::banner(false, false, Duration::ZERO, quiet));
        assert_eq!(coarse_said.as_deref(), Some("last update <1m ago"));
        assert_eq!(next, Some(Duration::from_secs(55)), "turns 1m at a minute");
        let retry = Duration::from_millis(4_400);
        assert_eq!(
            crate::banner(false, true, retry, Duration::ZERO).as_deref(),
            Some("daemon not answering · retrying in 4s")
        );
        let (said, next) = coarse(|| crate::banner(false, true, retry, Duration::ZERO));
        assert_eq!(
            said.as_deref(),
            Some("daemon not answering · retrying in <1m")
        );
        assert_eq!(
            next,
            Some(Duration::from_millis(3_900)),
            "gone when it rounds to no second"
        );
        assert_eq!(crate::ago(Duration::from_secs(12)), "12s");
        assert_eq!(coarse(|| crate::ago(Duration::from_secs(12))).0, "<1m");
        assert_eq!(crate::countdown(Duration::from_secs(48)), "0:48");
        assert_eq!(
            coarse(|| crate::countdown(Duration::from_secs(250))).0,
            "4m"
        );
        assert_eq!(
            crate::span_word(30),
            "30s",
            "a chart's span is not a time that moves"
        );
        assert_eq!(coarse(|| crate::span_word(30)).0, "30s");
    }
}
