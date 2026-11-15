//! The replay: each service's state, and whether the stack is up, walked through the journal in time order.
//!
//! Go added the time between two events to every service at every event. Here each service settles its own figures
//! only at its own events, against one running total of the stack's up-time, which gives the same sums in time
//! linear in events plus services.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use doze_display::{self as words, Kind};

use super::words::{cause_list, cause_word, event_tone, first_words, gb_hours, hours, stamp, took};
use super::{Activity, Ask, Ev, Line, Mark, Row, SpanBar};
use crate::overview::Look;
use crate::view::StackView;

/// One service's state and figures.
pub(super) struct State {
    awake: bool,
    since: SystemTime,
    ready: Option<SystemTime>,
    cause: String,
    awake_d: Duration,
    asleep_d: Duration,
    wakes: usize,
    crashes: usize,
    causes: HashMap<String, usize>,
    spans: Vec<SpanBar>,
    marks: Vec<Mark>,
    /// When its figures were last settled, and the stack's up-time then.
    settled: SystemTime,
    up_then: Duration,
    /// Beyond Go: each wake-to-ready in the span.
    readies: Vec<Duration>,
    /// The current stretch's wake-to-ready, as its ready recorded it.
    ready_took: Option<Duration>,
}

/// The replay's result: the services in the page's order, each one's state, the stack's up-time.
pub(super) struct Replay {
    names: Vec<String>,
    looks: HashMap<String, Look>,
    states: HashMap<String, State>,
    up_d: Duration,
}

/// The walk's clock: where it is, whether the stack is up, and for how long it has been in the span.
struct Walk {
    from: SystemTime,
    span: Duration,
    last: SystemTime,
    up: bool,
    up_d: Duration,
}

impl Walk {
    /// Counts the stack's up-time from the last event to `t` (Go `advance`, for the stack alone).
    fn advance(&mut self, t: SystemTime) {
        if t < self.from {
            return;
        }
        let a = self.last.max(self.from);
        if t > a && self.up {
            self.up_d += t.duration_since(a).unwrap_or_default();
        }
        self.last = t;
    }

    fn pos(&self, t: SystemTime) -> f64 {
        let d = t.duration_since(self.from).unwrap_or_default();
        (d.as_secs_f64() / self.span.as_secs_f64()).clamp(0.0, 1.0)
    }

    fn new_state(&self) -> State {
        State {
            awake: false,
            since: self.last,
            ready: None,
            cause: String::new(),
            awake_d: Duration::ZERO,
            asleep_d: Duration::ZERO,
            wakes: 0,
            crashes: 0,
            causes: HashMap::new(),
            spans: Vec::new(),
            marks: Vec::new(),
            settled: self.last,
            up_then: self.up_d,
            readies: Vec::new(),
            ready_took: None,
        }
    }

    /// Brings a service's figures up to `t`: awake time while awake, asleep time while the stack was up.
    fn settle(&self, s: &mut State, t: SystemTime) {
        if s.awake {
            let a = s.settled.max(self.from);
            s.awake_d += t.duration_since(a).unwrap_or_default();
        } else {
            s.asleep_d += self.up_d.saturating_sub(s.up_then);
        }
        s.settled = s.settled.max(t);
        s.up_then = self.up_d;
    }

    /// Ends an awake stretch at `at`, saying how (Go `closeSpan`).
    fn close(&self, s: &mut State, at: SystemTime, how: &str) {
        if !s.awake {
            return;
        }
        s.awake = false;
        if at < self.from {
            return;
        }
        let begin = s.since.max(self.from);
        let ago = doze_display::ago(at.duration_since(s.since).unwrap_or_default());
        let bar = SpanBar {
            from: self.pos(begin),
            to: self.pos(at),
            ready: s
                .ready
                .filter(|r| *r > self.from)
                .map_or(0.0, |r| self.pos(r)),
            title: format!("awake {ago} · woke {} · {how}", s.cause),
            open: false,
            took: s.ready_took,
        };
        s.spans.push(bar);
    }
}

/// The services: as the stack lists them (tasks have no sleep to count), then any the journal names that the stack no
/// longer declares; and how each looks now.
fn services(st: &StackView, events: &[Ev]) -> (Vec<String>, HashMap<String, Look>) {
    let snap = st.words();
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut looks = HashMap::new();
    for r in words::rows(&snap) {
        seen.insert(r.v.name.clone());
        if r.v.kind == Kind::Task {
            continue;
        }
        names.push(r.v.name.clone());
        looks.insert(r.v.name.clone(), Look::of(words::look(r.v)));
    }
    for e in events {
        if !e.instance.is_empty() && e.kind != "step" && seen.insert(e.instance.clone()) {
            names.push(e.instance.clone());
        }
    }
    (names, looks)
}

/// Walks the journal from `from` to now; fills the page's trouble and event lists on the way.
pub(super) fn replay(
    st: &StackView,
    events: &[Ev],
    from: SystemTime,
    ask: &Ask<'_>,
    out: &mut Activity,
) -> Replay {
    let (names, looks) = services(st, events);
    let start = events.first().map_or(from, |e| e.time.max(from));
    let mut walk = Walk {
        from,
        span: ask.span.length(),
        last: start,
        up: false,
        up_d: Duration::ZERO,
    };
    // Every service counts from the start, mentioned in the journal or not.
    let mut states: HashMap<String, State> = names
        .iter()
        .map(|n| (n.clone(), walk.new_state()))
        .collect();
    awake_before(st, events, &mut walk, &mut states);
    for e in events {
        walk.advance(e.time);
        if e.instance.is_empty() {
            continue;
        }
        let s = states
            .entry(e.instance.clone())
            .or_insert_with(|| walk.new_state());
        walk.settle(s, e.time);
        step(&mut walk, s, e, ask, out);
        listed(e, walk.from, ask, out);
    }
    // A running stack is up to now, whatever its journal last said.
    if st.running {
        walk.up = true;
    }
    walk.advance(ask.now);
    for n in &names {
        let s = states.entry(n.clone()).or_insert_with(|| walk.new_state());
        walk.settle(s, ask.now);
        if s.awake {
            still_awake(&walk, s, ask.now);
        }
    }
    Replay {
        names,
        looks,
        states,
        up_d: walk.up_d,
    }
}

/// A service awake now that the journal never saw wake has been awake since before it begins: it starts the replay
/// awake, and its stack up.
fn awake_before(
    st: &StackView,
    events: &[Ev],
    walk: &mut Walk,
    states: &mut HashMap<String, State>,
) {
    if !st.running {
        return;
    }
    let woke: HashSet<&str> = events
        .iter()
        .filter(|e| e.kind == "wake")
        .map(|e| e.instance.as_str())
        .collect();
    let snap = st.words();
    for v in &snap.instances {
        if v.kind != Kind::Task && words::awake_now(v) && !woke.contains(v.name.as_str()) {
            let mut s = walk.new_state();
            s.awake = true;
            s.cause = "before the journal begins".into();
            states.insert(v.name.clone(), s);
            walk.up = true;
        }
    }
}

/// One event's effect on its service and on whether the stack is up.
fn step(walk: &mut Walk, s: &mut State, e: &Ev, ask: &Ask<'_>, out: &mut Activity) {
    let inside = e.time >= walk.from;
    match e.kind.as_str() {
        "wake" => {
            walk.up = true;
            if !s.awake {
                (s.awake, s.since, s.ready) = (true, e.time, None);
                s.ready_took = None;
                s.cause = cause_word(&e.cause_kind, &e.cause_by);
                if inside {
                    s.wakes += 1;
                    *s.causes.entry(s.cause.clone()).or_default() += 1;
                }
            }
        }
        "ready" => {
            walk.up = true;
            s.ready = Some(e.time);
            let d = if e.took.is_zero() {
                e.time.duration_since(s.since).unwrap_or_default()
            } else {
                e.took
            };
            s.ready_took = s.awake.then_some(d);
            if inside && s.awake {
                s.readies.push(d);
            }
        }
        "sleep" => {
            let how = if e.cause_kind.is_empty() {
                "asked"
            } else {
                &e.cause_kind
            };
            walk.close(s, e.time, &format!("slept ({how})"));
            if e.cause_kind == "shutdown" {
                walk.up = false; // the stack is going down; it comes back with a wake
            }
        }
        "crash" | "failed" | "give_up" => trouble(walk, s, e, ask, out, inside),
        _ => {}
    }
}

/// A crash, a failed start or a give-up: the stretch ends, a mark, a line in Trouble.
fn trouble(walk: &Walk, s: &mut State, e: &Ev, ask: &Ask<'_>, out: &mut Activity, inside: bool) {
    let said = e.kind.replace('_', " ");
    walk.close(s, e.time, &said);
    if !inside {
        return;
    }
    s.crashes += 1;
    s.marks.push(Mark {
        at: walk.pos(e.time),
        kind: e.kind.clone(),
        title: format!(
            "{} {} {said}{}",
            super::words::weekday(e.time, ask.zone),
            super::words::hm(e.time, ask.zone),
            first_words(&e.detail)
        ),
    });
    let sentence = doze_display::sentence(&e.words());
    let (kind, rest) = sentence.split_once(' ').unwrap_or((&sentence, ""));
    out.trouble.push(Line {
        at: stamp(e.time, ask.span, ask.zone),
        kind: kind.to_owned(),
        text: format!("{} {rest}", e.instance)
            .trim_end_matches([':', ' '])
            .to_owned(),
        tone: "bad",
    });
}

/// The event as a line of What happened, when it is in the span and the page shows its service.
fn listed(e: &Ev, from: SystemTime, ask: &Ask<'_>, out: &mut Activity) {
    if e.time < from || (!ask.inst.is_empty() && e.instance != ask.inst) || e.kind == "step" {
        return;
    }
    let sentence = doze_display::sentence(&e.words());
    let (kind, rest) = sentence.split_once(' ').unwrap_or((&sentence, ""));
    out.events.push(Line {
        at: stamp(e.time, ask.span, ask.zone),
        kind: kind.to_owned(),
        text: format!("{} {rest}", e.instance),
        tone: event_tone(kind),
    });
}

/// A stretch still open at the end of the span.
fn still_awake(walk: &Walk, s: &mut State, now: SystemTime) {
    let begin = s.since.max(walk.from);
    let ago = doze_display::ago(now.duration_since(s.since).unwrap_or_default());
    s.spans.push(SpanBar {
        from: walk.pos(begin),
        to: 1.0,
        ready: s
            .ready
            .filter(|r| *r > walk.from)
            .map_or(0.0, |r| walk.pos(r)),
        title: format!("awake {ago} · woke {} · still awake", s.cause),
        open: true,
        took: s.ready_took,
    });
}

/// The rows and the totals (Go: the second half of `ActivityOf`).
pub(super) fn rows(r: &Replay, ask: &Ask<'_>, out: &mut Activity) {
    let (mut awake_all, mut asleep_all) = (Duration::ZERO, Duration::ZERO);
    let (mut freed_gbh, mut priced) = (0.0_f64, 0_usize);
    for n in &r.names {
        if !ask.inst.is_empty() && *n != ask.inst {
            continue;
        }
        let Some(s) = r.states.get(n) else { continue };
        let mut row = row(n, s, r.looks.get(n).copied());
        if let Some(b) = ask.ram.get(n).copied().filter(|b| *b > 0)
            && !s.asleep_d.is_zero()
        {
            #[allow(clippy::cast_precision_loss)] // WHY: bytes to GB, as Go's float64(b)
            let gbh = b as f64 / f64::from(1_u32 << 30) * s.asleep_d.as_secs_f64() / 3600.0;
            row.freed = format!("~{}", gb_hours(gbh));
            freed_gbh += gbh;
            priced += 1;
        }
        awake_all += s.awake_d;
        asleep_all += s.asleep_d;
        out.summary.wakes += s.wakes;
        out.summary.crashes += s.crashes;
        out.rows.push(row);
    }
    out.summary.up = format!("{} of {}", hours(r.up_d), hours(ask.span.length()));
    let total = awake_all + asleep_all;
    out.summary.asleep = if total.is_zero() {
        "—".into()
    } else {
        format!("{}%", percent(asleep_all, total))
    };
    if priced > 0 {
        out.summary.freed = format!("about {}", gb_hours(freed_gbh));
        out.summary.freed_note = format!(
            "each service's memory when last seen awake, times the hours it slept; {priced} of {} had a figure",
            out.rows.len()
        );
    }
    out.trouble.reverse();
    out.events.reverse();
    out.events.truncate(150);
    out.empty = out.events.is_empty() && r.up_d.is_zero();
}

/// `int(part/total*100+0.5)`.
fn percent(part: Duration, total: Duration) -> u64 {
    let p = part.as_secs_f64() / total.as_secs_f64() * 100.0 + 0.5;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // WHY: Go's int() of 0..100.5
    let n = p as u64;
    n
}

fn row(n: &str, s: &State, look: Option<Look>) -> Row {
    let total = s.awake_d + s.asleep_d;
    let part = if total.is_zero() {
        0.0
    } else {
        s.asleep_d.as_secs_f64() / total.as_secs_f64()
    };
    let never = s.asleep_d < Duration::from_secs(60) && !total.is_zero();
    Row {
        name: n.to_owned(),
        look: look.unwrap_or(Look {
            word: "gone",
            glyph: "·",
            tone: "dim",
        }),
        asleep_part: part,
        asleep: if never {
            "never slept".into()
        } else {
            format!(
                "{} asleep · {}%",
                hours(s.asleep_d),
                percent(s.asleep_d, total.max(Duration::from_nanos(1)))
            )
        },
        wakes: s.wakes,
        woken_by: cause_list(&s.causes),
        crashes: s.crashes,
        freed: String::new(),
        spans: s.spans.clone(),
        marks: s.marks.clone(),
        never,
        ready: typical(&s.readies),
    }
}

/// The median wake-to-ready, in words.
fn typical(readies: &[Duration]) -> String {
    let mut v = readies.to_vec();
    v.sort();
    v.get(v.len() / 2)
        .map(|d| match v.len() {
            1 => format!("ready in {}", took(*d)),
            _ => format!("ready in {}, typically", took(*d)),
        })
        .unwrap_or_default()
}
