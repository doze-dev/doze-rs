//! An instance's page (Go `view.InstanceOf`), as the Instance, Cache, Mailpit, Task, AppLogs and Crash boards lay it
//! out: the head line, the tabs, and the cards of the overview. Every word comes from `doze-display`; the app draws
//! this and derives no fact of its own.

use super::console::{self, Node};
use super::logs::{self, Line};
use super::parts::{self, Chart, Countdown, Crash, Kv, Pill, Proc, Recent, RunRow};
use super::side::{self, Side};
use super::time::Zone;
use crate::convert;
use crate::ring::LogRing;
use crate::view::StackView;
use doze_display::{self as words, Kind, Look, Tone};
use std::time::SystemTime;

/// One tab of the page: its id (`overview`, `logs`, `last run`, `web console`, `console:<id>`) and the title drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageTab {
    pub id: String,
    pub title: String,
}

/// One thing it boots first, with its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedChip {
    pub name: String,
    pub known: bool,
    pub look: Option<Look>,
    pub glyph: &'static str,
    pub task: bool,
}

/// The overview's charts, from the instance's meters.
#[derive(Debug, Clone, PartialEq)]
pub struct Vitals {
    pub ram: Chart,
    pub cpu: Chart,
    pub conns: Chart,
}

/// A console block a module adds to the overview, after the card named (`connect`, `awake`, `vitals`, `recent`).
#[derive(Debug, Clone, PartialEq)]
pub struct Extend {
    pub after: String,
    pub node: Node,
}

/// An instance's page.
#[derive(Debug, Clone, PartialEq, Default)]
#[allow(clippy::struct_excessive_bools)] // WHY: the head switch's four states, as Go's page carries them
pub struct InstancePage {
    pub stack: String,
    pub name: String,
    pub kind: Kind,
    pub engine: String,
    pub version: String,
    pub look: Option<Look>,
    /// The switch: on, turned off, paused in doze.hcl, pinned awake.
    pub on: bool,
    pub off: bool,
    pub paused: bool,
    pub pinned: bool,
    /// The line beside the name: `postgres 17 · awake for 14m · pid 612`.
    pub path: String,
    pub because: String,
    pub hint: Option<String>,
    pub up: String,
    pub pid: i64,
    pub run: String,
    pub connect: Vec<Kv>,
    pub holders: Vec<Pill>,
    pub sleeps: String,
    pub dozing: Option<Countdown>,
    /// The web console's address, when it has one.
    pub web: String,
    pub tabs: Vec<PageTab>,
    pub needs: Vec<NeedChip>,
    pub vitals: Option<Vitals>,
    pub recent: Vec<Recent>,
    pub runs: Vec<RunRow>,
    pub crash: Option<Crash>,
    pub extend: Vec<Extend>,
    pub procs: Vec<Proc>,
    pub side: Vec<Side>,
    /// The stack has no instance of that name.
    pub missing: bool,
}

impl InstancePage {
    /// Serving: awake or dozing, for the verbs that need it.
    pub fn serving(&self) -> bool {
        matches!(self.look, Some(Look::Awake | Look::Dozing))
    }

    /// A task.
    pub fn is_task(&self) -> bool {
        self.kind == Kind::Task
    }

    /// The address a browser opens (`o`), when it has one.
    pub fn url(&self) -> Option<&str> {
        self.connect
            .iter()
            .map(|r| r.copy.as_str())
            .find(|c| c.starts_with("http://") || c.starts_with("https://"))
    }

    /// What `y` copies: the URL or the address.
    pub fn copy_line(&self) -> Option<&str> {
        self.connect
            .iter()
            .find(|r| r.k == "URL" || r.k == "Address")
            .map(|r| r.copy.as_str())
            .filter(|c| !c.is_empty())
    }
}

/// The page of `name` in `st` at `now`. `ring` is the stack's log ring, for a crash's last lines and each run's
/// number in the logs.
pub fn instance_page(
    st: &StackView,
    name: &str,
    ring: Option<&LogRing>,
    now: SystemTime,
    zone: Zone,
) -> InstancePage {
    let snap = st.words();
    let mut out = InstancePage {
        stack: st.name.clone(),
        name: name.to_owned(),
        side: side::side(&snap),
        ..InstancePage::default()
    };
    let (Some(wire), Some(v)) = (st.instance(name), snap.find(name)) else {
        out.missing = true;
        return out;
    };
    let look = words::look(v);
    out.kind = v.kind;
    out.engine.clone_from(&wire.engine);
    out.version.clone_from(&wire.version);
    out.pid = wire.pid;
    out.run.clone_from(&wire.run);
    out.look = Some(look);
    out.on = words::switch_on(v);
    out.off = v.disabled && !v.paused;
    (out.paused, out.pinned) = (v.paused, v.keep_awake);
    out.because = words::because(v, &snap, now);
    out.hint = words::hint(v, &snap, now);
    if v.state.awake()
        && let Some(d) = v.started_at.and_then(|s| now.duration_since(s).ok())
    {
        out.up = words::ago(d);
    }
    out.path = parts::path_line(wire, v, look, &out.up, &out.because, now);
    if words::has_web(v) {
        out.web = words::web_url(v).to_owned();
    }
    let console = st.consoles.get(name);
    out.tabs = tabs(v, console.and_then(|c| c.words.as_deref()));
    out.connect = parts::with_direct(wire, parts::connect_rows(wire, v));
    out.extend = console
        .map(|c| {
            console::extensions(c)
                .into_iter()
                .map(|(after, node)| Extend { after, node })
                .collect()
        })
        .unwrap_or_default();
    (out.holders, out.sleeps) = parts::holders(v, st.idle_timeout);
    out.dozing = parts::countdown(v, st.idle_timeout, now);
    out.needs = needs(v, &snap);
    out.vitals = vitals(st, name, v);
    out.recent = parts::recent(&st.events, name, zone);
    if v.kind == Kind::Task {
        out.runs = parts::runs(&st.events, v, now, zone);
        if let Some(ring) = ring {
            parts::match_runs(&mut out.runs, &logs::runs_of(ring, name));
        }
    }
    if words::troubled(v) && v.kind != Kind::Task {
        let last = ring
            .map(|r| last_run(r, name, now, zone))
            .unwrap_or_default();
        out.crash = Some(parts::crash(v, now, last));
    }
    out.procs = parts::procs(wire);
    out
}

/// The tabs, every service opening on its overview, as the inside boards do; titles as the page writes them.
fn tabs(v: &words::Instance, spec: Option<&words::ConsoleSpec>) -> Vec<PageTab> {
    let mut tabs: Vec<PageTab> = words::tabs(v, spec)
        .into_iter()
        .map(|t| PageTab {
            id: t.id.into_owned(),
            title: t.title.into_owned(),
        })
        .collect();
    if v.kind != Kind::Task {
        match tabs.iter().position(|t| t.id == words::TAB_OVERVIEW) {
            None => tabs.insert(
                0,
                PageTab {
                    id: words::TAB_OVERVIEW.to_owned(),
                    title: "overview".to_owned(),
                },
            ),
            Some(at) if at > 0 => {
                let t = tabs.remove(at);
                tabs.insert(0, t);
            }
            Some(_) => {}
        }
    }
    tabs
}

/// The title a tab is drawn with (the page's `tabTitle`): `Overview`, `Logs`, `Runs · 3`, `Console`, the module's.
pub fn tab_title(t: &PageTab, page: &InstancePage) -> String {
    let runs = page.runs.len();
    match t.id.as_str() {
        words::TAB_OVERVIEW => "Overview".to_owned(),
        words::TAB_LAST_RUN if runs > 0 => "Last run".to_owned(),
        words::TAB_LAST_RUN => "Overview".to_owned(),
        words::TAB_LOGS if page.is_task() && runs > 0 => format!("Runs · {runs}"),
        words::TAB_LOGS if page.is_task() => "Runs · none yet".to_owned(),
        words::TAB_LOGS => "Logs".to_owned(),
        words::TAB_WEB => "Console".to_owned(),
        _ => t.title.clone(),
    }
}

fn needs(v: &words::Instance, snap: &words::Snapshot) -> Vec<NeedChip> {
    words::needs(v, snap)
        .into_iter()
        .map(|n| NeedChip {
            name: n.name.to_owned(),
            known: n.known.is_some(),
            look: n.known.map(|k| k.look),
            glyph: n.glyph().unwrap_or("?"),
            task: snap.find(n.name).is_some_and(|o| o.kind == Kind::Task),
        })
        .collect()
}

/// Figures only while it serves: a crashing or waking process's last numbers would read as live.
fn vitals(st: &StackView, name: &str, v: &words::Instance) -> Option<Vitals> {
    let s = st.series.get(name).filter(|s| !s.is_empty())?;
    if !words::awake_now(v) {
        return None;
    }
    let mut cpu = parts::chart(&s.cpu, words::pct, "of one core");
    if let Some(&last) = s.cpu.last() {
        cpu.now = words::cores(last);
        cpu.sub = format!("cores · {} of one", words::pct(last));
    }
    Some(Vitals {
        ram: parts::chart(&s.ram, words::bytes_label, "resident"),
        cpu,
        conns: parts::chart(&s.conns, words::count, "connections"),
    })
}

/// Its last run's tail: what it said before it exited, from that run only, at most eight lines.
fn last_run(ring: &LogRing, name: &str, now: SystemTime, zone: Zone) -> Vec<Line> {
    let run = logs::runs_of(ring, name).last().copied().unwrap_or(0);
    let mut lines: Vec<Line> = ring
        .of(name)
        .filter(|l| run == 0 || l.run == run)
        .map(|l| logs::line_of(&l, false, now, zone))
        .collect();
    let extra = lines.len().saturating_sub(8);
    lines.drain(..extra);
    lines
}

/// The tone a need's glyph is drawn in.
pub fn need_tone(n: &NeedChip) -> Tone {
    n.look.map_or(Tone::Dim, Look::tone)
}

/// The instance as the words read it, for the app's own small questions (is it a module, an app).
pub fn words_of(st: &StackView, name: &str) -> Option<words::Instance> {
    st.instance(name).map(convert::instance)
}
