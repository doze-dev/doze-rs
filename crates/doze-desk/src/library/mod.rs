//! The Library, the app's front door (Go `view/library.go` and the parts of `serve/library.go` it is filled from):
//! every stack doze knows, running first with live figures, stopped below with when they last ran; the getting-started
//! strip while it is still useful; what other doze programs run here; the machine line.

mod addr;
mod machine;
mod others;

use addr::addresses;
pub use addr::{Addr, fronted, unready};
pub use machine::{Check, Doctor, fix_of, machine, names_off, parse_doctor, zone_off};
pub use others::{Names, Other, others};

use std::path::Path;
use std::time::SystemTime;

use doze_display as words;
use serde::Serialize;

use crate::conn::Conn;
use crate::overview::Shape;
use crate::view::StackView;

/// The Library.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub stacks: Vec<LibStack>,
    /// The head's line: `4 stacks · 2 up`.
    pub counts: String,
    /// The getting-started strip; empty once dismissed or all three are done.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    /// The line at the bottom: each part of the machine's setup and whether it is in place.
    pub machine: Vec<Check>,
    /// Services other doze programs run here.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<Other>,
}

/// One row.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibStack {
    /// The stack's files went away: `the folder is gone`, `doze.hcl is gone`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub missing: String,
    /// Its doze.hcl does not load: `doze.hcl does not load · line 9`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub broken: String,
    pub key: String,
    pub name: String,
    pub dir: String,
    pub running: bool,
    pub live: bool,
    /// `g` up and well, `r` something needs you, `y` up but not answering, empty stopped.
    pub dot: &'static str,
    /// What doze.hcl declares, tasks aside.
    pub declared: usize,
    /// `4 awake · 2 waking · 4 asleep`, or `stopped`, in toned runs.
    pub fig: Vec<Bit>,
    /// The dim line under it.
    pub sub: String,
    /// The red pill: `search is crashing · try 2 of 5`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub need: String,
    pub addresses: Vec<Addr>,
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    /// `up 14m`, or `up 3d ago`.
    pub when: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub last_set: Vec<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// A run of text in a tone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Bit {
    pub text: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub tone: &'static str,
}

/// One of the getting-started strip's three.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Step {
    pub title: &'static str,
    pub detail: String,
    pub done: bool,
    pub now: bool,
}

/// What the strip needs from prefs: dismissed, and whether a connection was ever seen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hints {
    pub dismissed: bool,
    pub connected: bool,
}

/// The Library of `stacks` (running first, as the hub lists them) at `now`. `shapes` gives a stopped stack's declared
/// instances when its doze.hcl was read.
pub fn library(
    stacks: &[StackView],
    shapes: &dyn Fn(&StackView) -> Option<Shape>,
    hints: Hints,
    now: SystemTime,
) -> Library {
    let mut lib = Library::default();
    let (mut up, mut any_up, mut any_conn) = (0, false, hints.connected);
    for st in stacks {
        lib.stacks.push(row(st, shapes(st).as_ref(), now));
        up += usize::from(st.running);
        any_up |= st.running || st.last_up.is_some();
        any_conn |= st.instances.iter().any(|v| v.conns > 0);
    }
    let n = stacks.len();
    lib.counts = match (n, up) {
        (0, _) => "no stacks yet".to_owned(),
        (n, 0) => format!("{n} stack{} · none up", words::plural(n as u64)),
        (n, up) => format!("{n} stack{} · {up} up", words::plural(n as u64)),
    };
    if !hints.dismissed && (n == 0 || !any_up || !any_conn) {
        lib.steps = steps(stacks.first(), any_up, any_conn);
    }
    lib
}

fn steps(first: Option<&StackView>, any_up: bool, any_conn: bool) -> Vec<Step> {
    let step = |title, detail: &str| Step {
        title,
        detail: detail.to_owned(),
        done: false,
        now: false,
    };
    let mut s = [
        step("Open a project", "a folder with your code, or an empty one"),
        step(
            "Bring it up",
            "one switch; doze starts what the stack needs, in order",
        ),
        step(
            "Connect to it",
            "every service gets a name: db.shop.doze, web.shop.doze",
        ),
    ];
    if let Some(f) = first {
        s[0].done = true;
        s[0].detail = format!(
            "{}, from {}",
            f.name,
            words::tilde(&f.dir, words::home().as_deref())
        );
    }
    if any_up {
        s[1].done = true;
        s[1].detail = "the rest wake when something connects".to_owned();
    }
    s[2].done = any_conn;
    if let Some(f) = first.filter(|f| f.running)
        && let Some(ex) = connect_example(f)
    {
        s[2].detail = ex;
    }
    if let Some(next) = s.iter_mut().find(|s| !s.done) {
        next.now = true;
    }
    s.to_vec()
}

/// The third step's detail for a running stack: the first two things one could connect to.
fn connect_example(st: &StackView) -> Option<String> {
    let snap = st.words();
    let parts: Vec<String> = st
        .instances
        .iter()
        .zip(&snap.instances)
        .filter(|(v, _)| v.kind != "task")
        .filter_map(|(v, i)| {
            let addr = words::reach(i)
                .map(|r| r.addr.into_owned())
                .unwrap_or_default();
            if v.kind == "module" && !v.domain.is_empty() {
                Some(format!("connect to {}", v.domain))
            } else if addr.starts_with("http") {
                Some(format!("open {addr}"))
            } else {
                None
            }
        })
        .take(2)
        .collect();
    (!parts.is_empty()).then(|| format!("{} · or copy an address from any row", parts.join(" · ")))
}

/// One stack's row (Go `libStackOf`).
fn row(st: &StackView, shape: Option<&Shape>, now: SystemTime) -> LibStack {
    let name = if st.name.is_empty() && !st.dir.is_empty() {
        Path::new(&st.dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        st.name.clone()
    };
    let snap = st.words();
    let t = words::tally(&snap.instances);
    let mut row = LibStack {
        key: st.key.dir().display().to_string(),
        name,
        dir: words::tilde(&st.dir, words::home().as_deref()).into_owned(),
        running: st.running,
        live: st.link.conn == Conn::Live,
        missing: missing_files(&st.dir),
        declared: snap
            .instances
            .iter()
            .filter(|i| i.kind != words::Kind::Task)
            .count(),
        last_set: st.last_set.clone(),
        ..LibStack::default()
    };
    row.need = need(st, &snap, now);
    if st.running {
        row.fig = fig(&t, &snap.instances);
        row.sub = conn_word(t.conns);
        if t.ram > 0 {
            row.sub = format!(
                "{} · {}",
                words::mb(i64::try_from(t.ram).unwrap_or(i64::MAX)),
                row.sub
            );
        }
        let since = st
            .since
            .and_then(|s| now.duration_since(s).ok())
            .unwrap_or_default();
        row.when = format!("up {}", words::ago(since));
        row.dot = if t.trouble + t.failed > 0 {
            "r"
        } else if st.link.conn != Conn::Live {
            "y"
        } else {
            "g"
        };
        (row.addresses, row.more) = addresses(st);
    } else {
        stopped_row(&mut row, st, shape, now);
    }
    row
}

fn stopped_row(row: &mut LibStack, st: &StackView, shape: Option<&Shape>, now: SystemTime) {
    row.fig = vec![Bit {
        text: crate::overview::Look::STOPPED.word.into(),
        tone: "dim",
    }];
    if let Some(s) = shape {
        row.declared = s.decls.iter().filter(|d| d.kind != "task").count();
        if let Some(err) = &s.error {
            let ce =
                crate::overview::config_error(&Path::new(&st.dir).join("doze.hcl"), err, None, now);
            row.broken = if ce.line > 0 {
                format!("doze.hcl does not load · line {}", ce.line)
            } else {
                "doze.hcl does not load".to_owned()
            };
        }
    }
    row.sub = if !st.last_set.is_empty() {
        format!("last time: {}", st.last_set.join(", "))
    } else if st.last_up.is_some() {
        "last time: everything".to_owned()
    } else {
        "never started here".to_owned()
    };
    row.when = match st.last_up {
        Some(t) => format!(
            "up {} ago",
            words::ago(now.duration_since(t).unwrap_or_default())
        ),
        None => "never up".to_owned(),
    };
}

/// The red pill: the first instance in trouble, or a daemon that stopped answering.
fn need(st: &StackView, snap: &words::Snapshot, now: SystemTime) -> String {
    if let Some(i) = snap.instances.iter().find(|i| words::troubled(i)) {
        // "backfill failed", as Needs you says it, not "backfill is failed"; "gateway is crashing".
        let mut s = match words::look(i) {
            words::Look::Failed => format!("{} {}", i.name, words::Look::Failed.word()),
            l => format!("{} is {}", i.name, l.word()),
        };
        let b = words::because(i, snap, now);
        if !b.is_empty() {
            s = format!("{s} · {b}");
        }
        return s;
    }
    if st.running && st.link.conn == Conn::Down {
        return "daemon not answering · retrying".to_owned();
    }
    String::new()
}

/// The running figure: how many read as each state, tasks aside, in the words every other surface uses. Go folded
/// paused into off and failed (a task's too) into crashing, so the Library disagreed with the stack's own strip. Trouble
/// leads, so a narrow column cuts the quiet words and not it.
fn fig(t: &words::Tally, ins: &[words::Instance]) -> Vec<Bit> {
    use crate::overview::count;
    use words::Look;
    let mut out: Vec<Bit> = Vec::new();
    for (n, look, tone) in [
        (count(ins, Look::Crashing), Look::Crashing, "bad"),
        (count(ins, Look::Failed), Look::Failed, "bad"),
        (t.awake, Look::Awake, "ok"),
        (t.waking, Look::Waking, "cool"),
        (t.dozing, Look::Dozing, "warn"),
        (t.asleep, Look::Asleep, ""),
        (t.off, Look::Off, "dim"),
        (t.paused, Look::Paused, "dim"),
    ] {
        let word = look.word();
        if n == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push(Bit {
                text: " · ".into(),
                tone: "",
            });
        }
        out.push(Bit {
            text: format!("{n} {word}"),
            tone,
        });
    }
    if out.is_empty() {
        out.push(Bit {
            text: "nothing awake".into(),
            tone: "dim",
        });
    }
    out
}

fn conn_word(n: u64) -> String {
    match n {
        0 => "no connections".to_owned(),
        1 => "1 connection".to_owned(),
        n => format!("{n} connections"),
    }
}

/// What of a stack's files is gone, or empty when its doze.hcl is there (or its folder is not known).
pub fn missing_files(dir: &str) -> String {
    let dir = Path::new(dir);
    if !dir.is_absolute() || dir.join("doze.hcl").exists() {
        String::new()
    } else if !dir.exists() {
        "the folder is gone".to_owned()
    } else {
        "doze.hcl is gone".to_owned()
    }
}
