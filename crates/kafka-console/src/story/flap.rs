//! The flapping rule: a group is flapping when a member keeps timing out and coming back. Three or more
//! expired-then-joined cycles in ten minutes. A member that leaves cleanly never counts: it told the broker it was going,
//! and nothing ran out. The code and the words of the Flapping page are this one function.

use super::words::{lasted, member_label};
use crate::format::clock;
use crate::model::{StoryCause, StoryFlap, StoryGen, StoryRule};

/// How many cycles make flapping.
pub const NEEDS: usize = 3;
/// The window the cycles are counted in.
pub const WINDOW_MS: u64 = 10 * 60 * 1000;
/// A member that joined and left within this is worth a look, though it is not flapping.
const QUICK_MS: u64 = 6 * 60 * 1000;

/// Whom a cycle is about: the client id, or none (then any member that joins after an expiry is the one back).
fn who_key(g: &StoryGen) -> Option<&str> {
    g.who
        .as_ref()
        .and_then(|w| w.client.as_deref())
        .filter(|c| !c.is_empty())
}

/// The cycles (a session expiry, then the same client joining) among `gens`, oldest first, that began at or after `since`.
/// Each expiry is used once.
#[must_use]
pub fn cycles(gens: &[StoryGen], since: i64) -> usize {
    let mut open: Vec<Option<&str>> = Vec::new();
    let mut n = 0;
    for g in gens.iter().filter(|g| g.at_ms >= since) {
        match g.cause {
            StoryCause::SessionExpired => open.push(who_key(g)),
            StoryCause::MemberJoined => {
                if let Some(at) = open.iter().position(|k| *k == who_key(g)) {
                    open.remove(at);
                    n += 1;
                }
            }
            _ => {}
        }
    }
    n
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn who(g: &StoryGen) -> String {
    g.who
        .as_ref()
        .map_or_else(|| "a member".to_owned(), |w| member_label(&w.member))
}

/// A member that joined and, within [`QUICK_MS`], left: the latest such, with how long it stayed.
fn quick_stay(gens: &[StoryGen]) -> Option<(&StoryGen, &StoryGen, u64)> {
    gens.iter()
        .filter(|j| j.cause == StoryCause::MemberJoined)
        .filter_map(|j| {
            let me = j.who.as_ref()?.member.as_str();
            gens.iter()
                .find(|l| {
                    l.cause == StoryCause::MemberLeft
                        && l.at_ms >= j.at_ms
                        && l.who.as_ref().is_some_and(|w| w.member == me)
                })
                .map(|l| (j, l, u64::try_from(l.at_ms - j.at_ms).unwrap_or(0)))
        })
        .rfind(|(_, _, stay)| *stay <= QUICK_MS)
}

/// What the last ten minutes held, by cause: how many and the time of the newest.
struct Window<'a> {
    in_it: Vec<&'a StoryGen>,
}

impl Window<'_> {
    fn count(&self, c: StoryCause) -> usize {
        self.in_it.iter().filter(|g| g.cause == c).count()
    }

    /// `1 join (22:09:02)`, `3 joins (last 22:09:02)`, `0 expiries`.
    fn said(&self, c: StoryCause, one: &str, many: &str) -> String {
        let n = self.count(c);
        match self.in_it.iter().rev().find(|g| g.cause == c) {
            Some(g) if n == 1 => format!("{} ({})", plural(n, one, many), clock(g.at_ms)),
            Some(g) => format!("{} (last {})", plural(n, one, many), clock(g.at_ms)),
            None => plural(n, one, many),
        }
    }
}

fn cycle_row(found: usize, w: &Window<'_>) -> StoryRule {
    StoryRule {
        ok: found < NEEDS,
        title: format!("Expired, then joined, {NEEDS} times in 10 minutes"),
        detail: format!(
            "Last 10 minutes: {}, {}, {}.",
            w.said(StoryCause::MemberJoined, "join", "joins"),
            w.said(StoryCause::MemberLeft, "clean leave", "clean leaves"),
            w.said(StoryCause::SessionExpired, "expiry", "expiries"),
        ),
        value: plural(found, "cycle", "cycles"),
    }
}

fn leave_row(gens: &[StoryGen]) -> StoryRule {
    let detail = match gens
        .iter()
        .rev()
        .find(|g| g.cause == StoryCause::MemberLeft)
    {
        Some(g) => format!(
            "{} sent LeaveGroup. The broker was told; nothing ran out.",
            who(g)
        ),
        None => {
            "A member that says it is going sends LeaveGroup. The broker is told; nothing runs out."
                .to_owned()
        }
    };
    StoryRule {
        ok: true,
        title: "A leave is not a timeout".to_owned(),
        detail,
        value: "not counted".to_owned(),
    }
}

fn expiry_row(gens: &[StoryGen], now_ms: i64) -> StoryRule {
    let expired: Vec<&StoryGen> = gens
        .iter()
        .filter(|g| g.cause == StoryCause::SessionExpired)
        .collect();
    let span = gens.first().map_or_else(
        || "what the ring holds".to_owned(),
        |g| {
            format!(
                "the last {}",
                lasted(u64::try_from(now_ms.saturating_sub(g.at_ms)).unwrap_or(0))
            )
        },
    );
    let names: Vec<String> = expired
        .iter()
        .rev()
        .take(3)
        .map(|g| format!("{} at {}", who(g), clock(g.at_ms)))
        .collect();
    let detail = match (expired.len(), names.first()) {
        (0, _) | (_, None) => "None.".to_owned(),
        (1, Some(only)) => format!("{only} only."),
        (n, _) if n > 3 => format!("{} and earlier ones.", names.join(", ")),
        _ => format!("{}.", crate::words::list_and(&names)),
    };
    StoryRule {
        ok: expired.len() < NEEDS,
        title: format!("Session expiries in {span}"),
        detail,
        value: expired.len().to_string(),
    }
}

fn quick_row(gens: &[StoryGen], found: usize) -> StoryRule {
    match quick_stay(gens) {
        Some((joined, left, stay)) => StoryRule {
            ok: false,
            title: "A member joined and left within 6 minutes".to_owned(),
            detail: format!(
                "{}, {} to {}. {}",
                who(joined),
                clock(joined.at_ms),
                clock(left.at_ms),
                if found >= NEEDS {
                    "Part of the pattern."
                } else {
                    "Not flapping; watch it."
                }
            ),
            value: lasted(stay),
        },
        None => StoryRule {
            ok: true,
            title: "No member joined and left within 6 minutes".to_owned(),
            detail: "A member that comes and goes quickly is worth a look; none has.".to_owned(),
            value: "none".to_owned(),
        },
    }
}

/// The rule held against a group's generations (oldest first) at `now_ms`.
#[must_use]
pub fn flap_of(gens: &[StoryGen], now_ms: i64) -> StoryFlap {
    let since = now_ms.saturating_sub(i64::try_from(WINDOW_MS).unwrap_or(i64::MAX));
    let found = cycles(gens, since);
    let window = Window {
        in_it: gens.iter().filter(|g| g.at_ms >= since).collect(),
    };
    let rows = vec![
        cycle_row(found, &window),
        leave_row(gens),
        expiry_row(gens, now_ms),
        quick_row(gens, found),
    ];
    StoryFlap {
        flapping: found >= NEEDS,
        cycles: found,
        needs: NEEDS,
        window_ms: WINDOW_MS,
        expiries_in_window: window.count(StoryCause::SessionExpired),
        expiries: gens
            .iter()
            .filter(|g| g.cause == StoryCause::SessionExpired)
            .count(),
        rows,
    }
}
