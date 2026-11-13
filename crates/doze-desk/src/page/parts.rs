//! The parts of an instance's page: the head's path line, the connect card's rows, why it is awake, the countdown,
//! the charts, the recent events, a task's runs, a crash.

use super::Ink;
use super::logs::Line;
use super::time;
use doze_control as wire;
use doze_display::{self as words, Holder, Kind, Look, State};
use std::time::{Duration, SystemTime};

pub use super::recent::recent;
pub use super::runs::{RunRow, match_runs, runs};

/// One row of a card: a key, a value, and what the copy button copies (nothing for no button). The instance page's
/// and the List pane's; it serializes as Go's `view.KV`.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct Kv {
    pub k: String,
    pub v: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub copy: String,
    pub mono: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dim: bool,
    /// A line under the value: why it reads as it does.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Kv {
    fn mono(k: &str, v: &str, copy: &str) -> Kv {
        Kv {
            k: k.to_owned(),
            v: v.to_owned(),
            copy: copy.to_owned(),
            mono: true,
            ..Kv::default()
        }
    }
}

/// One reason it is awake, and a word on it.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct Pill {
    pub text: String,
    pub explain: String,
}

/// How long until a dozing instance sleeps: the clock left and the share of the idle time gone, for the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Countdown {
    pub left: String,
    pub fraction: f64,
}

/// One series ready to draw: the readings oldest first, the scale, and the figures beside it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chart {
    pub points: Vec<f64>,
    pub lo: f64,
    pub hi: f64,
    /// The latest reading, as the card's figure; `sub` the words beside it.
    pub now: String,
    pub sub: String,
    pub peak: String,
    pub mean: String,
    /// How long the points cover (`4m`).
    pub span: String,
}

/// One of doze's events for the instance: `woke` `(asked for)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    pub at: String,
    pub kind: String,
    pub text: String,
    pub ink: Ink,
}

/// What the Crash board shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Crash {
    pub error: String,
    /// The rest of the error after its first line (the frame, the file and line).
    pub r#where: String,
    /// `try 3 of 5`.
    pub tries: String,
    /// The last try as a word: `fifth`.
    pub last_try: String,
    pub fraction: f64,
    /// `4 s`.
    pub next_in: String,
    pub gave_up: bool,
    /// The port its exit says was taken, if it says one (PortClash).
    pub port: Option<u16>,
    /// What it said before it exited.
    pub last_run: Vec<Line>,
}

/// One process under an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub cmd: String,
    pub pid: i64,
    pub mem: String,
    pub depth: i64,
}

/// The engine as the page names it: a module's engine and version, `task`, else the engine or `process`.
pub fn engine(v: &wire::InstanceView) -> String {
    match v.kind.as_str() {
        "module" => format!(
            "{} {}",
            v.engine,
            v.version.strip_prefix("builtin").unwrap_or(&v.version)
        )
        .trim()
        .to_owned(),
        "task" => "task".to_owned(),
        _ if v.engine.is_empty() => "process".to_owned(),
        _ => v.engine.clone(),
    }
}

/// The line beside the name on the head. (Go `pathLine`.)
pub fn path_line(
    wire: &wire::InstanceView,
    v: &words::Instance,
    look: Look,
    up: &str,
    because: &str,
    now: SystemTime,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    match v.kind {
        Kind::Task => {
            parts.push("task".to_owned());
            if !v.run.is_empty() {
                parts.push(words::short_run(&v.run).into_owned());
            }
            match &v.task {
                _ if look == Look::Running => parts.push("running now".to_owned()),
                Some(t) => {
                    let ago = t
                        .ran_at
                        .and_then(|r| now.duration_since(r).ok())
                        .unwrap_or_default();
                    parts.push(format!("last run {} ago", words::ago(ago)));
                    parts.push(format!("exit {} in {}", t.exit_code, time::tenths(t.took)));
                }
                None => parts.push("not run yet".to_owned()),
            }
        }
        Kind::Module => {
            parts.push(engine(wire));
            parts.push(if up.is_empty() {
                look.word().to_owned()
            } else {
                up_phrase(look, up, " for ")
            });
            if wire.pid > 0 && v.state.awake() {
                parts.push(format!("pid {}", wire.pid));
            }
        }
        _ => {
            parts.push(engine(wire));
            if !v.run.is_empty() {
                parts.push(words::short_run(&v.run).into_owned());
            }
            parts.push(if words::troubled(v) || up.is_empty() {
                look.word().to_owned()
            } else {
                up_phrase(look, up, " ")
            });
        }
    }
    if !v.state.awake() && !words::troubled(v) && v.kind != Kind::Task {
        parts.push(because.to_owned());
    }
    parts.join(" · ")
}

/// The connect card's rows, as the Instance board lists them. (Go `connectRows`.)
pub fn connect_rows(wire: &wire::InstanceView, v: &words::Instance) -> Vec<Kv> {
    let mut rows = Vec::new();
    if !wire.https.is_empty() {
        rows.push(Kv::mono("Domain", &wire.https, &wire.https));
        for d in wire.domains.iter().filter(|d| d.starts_with("*.")) {
            rows.push(Kv {
                dim: true,
                ..Kv::mono(
                    "Subdomains",
                    &format!("https://{d} · any name under it"),
                    "",
                )
            });
        }
    }
    if !wire.url.is_empty() {
        rows.push(Kv::mono("URL", &wire.url, &wire.url));
    }
    if !wire.env_var.is_empty() {
        let copy = format!("export {}={}", wire.env_var, wire.url);
        rows.push(Kv::mono("Env var", &wire.env_var, &copy));
    }
    if let Some(a) = words::address(v) {
        rows.push(Kv::mono("Address", &a, &a));
    }
    if !wire.data_dir.is_empty() {
        let home = words::home();
        let data = words::tilde(&wire.data_dir, home.as_deref());
        rows.push(Kv {
            dim: true,
            ..Kv::mono("Data", &data, "")
        });
    }
    rows
}

/// The connect rows with the process's own IP and port under its address, as the Go app's service adds it (Go
/// `Service.reach`, without the front door's URL, which needs the names registry).
pub fn with_direct(wire: &wire::InstanceView, rows: Vec<Kv>) -> Vec<Kv> {
    let ep = wire.endpoint.as_str();
    if ep.is_empty() || ep.starts_with("unix:") {
        return rows;
    }
    let is_ip = |hp: &str| {
        hp.rsplit_once(':').is_some_and(|(h, _)| {
            h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok()
        })
    };
    let direct = if is_ip(&wire.bind) && !is_ip(ep) {
        wire.bind.as_str()
    } else {
        ep
    };
    let mut out = Vec::with_capacity(rows.len() + 1);
    for r in rows {
        let address = r.k == "Address" && r.v != direct;
        out.push(r);
        if address {
            out.push(Kv {
                dim: true,
                note: "the process itself, without a name".to_owned(),
                ..Kv::mono("IP and port", direct, direct)
            });
        }
    }
    out
}

/// Why it is awake, as pills with a word on each, and the sentence under them. (Go `holders`.)
pub fn holders(v: &words::Instance, idle: Duration) -> (Vec<Pill>, String) {
    let pills = pills(v);
    if v.always_on {
        return (
            pills,
            "It runs until stopped: it has no port, so nothing can hold it, and it never sleeps on its own."
                .to_owned(),
        );
    }
    sentence(v, idle, pills)
}

/// What holds it, a pill each.
fn pills(v: &words::Instance) -> Vec<Pill> {
    let pill = |text: String, explain: &str| Pill {
        text,
        explain: explain.to_owned(),
    };
    v.holders
        .iter()
        .filter_map(|h| match h {
            Holder::Dependant(n) => Some(pill(
                format!("{n} needs it"),
                "a dependant that's awake keeps it awake",
            )),
            Holder::Connections(n) => Some(pill(
                format!("{n} connection{}", words::plural(u64::from(*n))),
                "open clients, through doze's proxy",
            )),
            Holder::Pin => Some(pill(
                "pinned awake".to_owned(),
                "stays up past the idle time until unpinned",
            )),
            Holder::Other => None,
        })
        .collect()
}

/// The sentence under the pills: what keeps it up, or why nothing does.
fn sentence(v: &words::Instance, idle: Duration, pills: Vec<Pill>) -> (Vec<Pill>, String) {
    let after = if idle.is_zero() {
        String::new()
    } else {
        format!(" after {}", words::clock(idle))
    };
    // Off and paused do not doze, and a start under way is not idle: Go told all three the dozing sentence.
    if v.paused {
        return (pills, "enabled = false in doze.hcl: it does not run, and nothing wakes it until the file says so.".to_owned());
    }
    if v.disabled {
        return (
            pills,
            "Turned off: nothing wakes it, not even a connection, until you switch it on."
                .to_owned(),
        );
    }
    if pills.is_empty() && v.state == State::Booting {
        return (
            pills,
            format!(
                "It is starting. Once it answers, what connects keeps it awake; with nothing, it sleeps{after}."
            ),
        );
    }
    if pills.is_empty() {
        if v.state == State::Idle {
            return (
                pills,
                "nothing: it sleeps when the idle time is up".to_owned(),
            );
        }
        return (
            pills,
            format!(
                "No connection right now. It dozes, then sleeps{after} if nothing connects; the next connection wakes it."
            ),
        );
    }
    (
        pills,
        format!("With neither, it sleeps{after}. Its data stays; the next connection wakes it."),
    )
}

/// The countdown while it dozes.
pub fn countdown(v: &words::Instance, idle: Duration, now: SystemTime) -> Option<Countdown> {
    if v.state != State::Idle || idle.is_zero() {
        return None;
    }
    let gone = now.duration_since(v.idle_since?).unwrap_or_default();
    let left = idle.checked_sub(gone).filter(|l| !l.is_zero())?;
    Some(Countdown {
        left: words::clock(left),
        fraction: part(gone.as_secs_f64(), idle.as_secs_f64()),
    })
}

/// `v / sum`, to three places. (Go `part`.)
pub fn part(v: f64, sum: f64) -> f64 {
    if sum <= 0.0 {
        return 0.0;
    }
    (v / sum * 1000.0 + 0.5).floor() / 1000.0
}

/// One series as a chart.
pub fn chart(vs: &[f64], label: fn(f64) -> String, sub: &str) -> Chart {
    let (peak, mean) = words::stats(vs);
    let (lo, hi) = words::scale(vs, 0.0, false);
    Chart {
        points: vs.to_vec(),
        lo,
        hi,
        now: vs.last().map(|&v| label(v)).unwrap_or_default(),
        sub: sub.to_owned(),
        peak: label(peak),
        mean: label(mean),
        span: words::span_word(vs.len() as u64),
    }
}

/// The crash card: the error, the tries, the next try; the port its exit says was taken.
pub fn crash(v: &words::Instance, now: SystemTime, last_run: Vec<Line>) -> Crash {
    let error = words::first_line(&v.last_error).to_owned();
    let r#where = v
        .last_error
        .split_once('\n')
        .map(|(_, rest)| rest.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let mut c = Crash {
        error,
        r#where,
        tries: String::new(),
        last_try: String::new(),
        fraction: 0.0,
        next_in: String::new(),
        gave_up: v.state == State::Failed || v.state == State::Error,
        port: None,
        last_run,
    };
    if v.restart_max > 0 {
        c.tries = format!("try {} of {}", v.restart_count, v.restart_max);
        c.last_try = ordinal(v.restart_max);
        c.fraction = part(f64::from(v.restart_count), f64::from(v.restart_max));
    }
    // Under half a second it is a moment, not "0 s".
    if let Some(left) = v.next_restart.and_then(|n| n.duration_since(now).ok())
        && left.as_millis() >= 500
    {
        c.next_in = format!("{} s", (left.as_secs_f64() + 0.5).floor());
    }
    let texts = [c.error.as_str(), c.r#where.as_str()]
        .into_iter()
        .chain(c.last_run.iter().map(|l| l.text.as_str()));
    c.port = crate::ports::port_in_use(texts);
    c
}

/// A small count as a word (`fifth`); larger ones as `10th`.
fn ordinal(n: u32) -> String {
    const WORDS: [&str; 10] = [
        "", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
    ];
    match WORDS.get(n as usize) {
        Some(w) if n > 0 => (*w).to_owned(),
        _ => format!("{n}th"),
    }
}

/// The processes under it.
pub fn procs(v: &wire::InstanceView) -> Vec<Proc> {
    v.children
        .iter()
        .map(|p| Proc {
            cmd: p.cmd.clone(),
            pid: p.pid,
            mem: words::mb(p.rss),
            depth: p.depth,
        })
        .collect()
}

/// How long it has been up, in its own word: "awake 14m", or "waking for 7m" for one still starting (a wake that has
/// not finished is not awake, though its process is up).
fn up_phrase(look: Look, up: &str, sep: &str) -> String {
    if look == Look::Waking {
        format!("{} for {up}", look.word())
    } else {
        format!("{}{sep}{up}", Look::Awake.word())
    }
}
