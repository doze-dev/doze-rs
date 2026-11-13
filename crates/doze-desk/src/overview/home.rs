//! A running stack's overview (Go `view.HomeOf`): the four figures, every instance under its group in the board's
//! columns, and the Grid.

use std::collections::HashMap;
use std::time::SystemTime;

use doze_control::InstanceView;
use doze_display as words;
use serde::Serialize;

use super::config::{ConfigChanged, ConfigError};
use super::grid::{GridView, grid};
use super::stopped::Stopped;
use super::vitals::{Vitals4, vitals};
use super::{Banner, Look};
use crate::conn::Conn;
use crate::view::StackView;

/// A stack's overview.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    /// The stack's hub key, so a page can tell this stack's page from the last one's.
    pub key: String,
    pub stack: String,
    /// Its folder, with `~` for the home directory.
    pub dir: String,
    pub running: bool,
    pub banner: Banner,
    /// The running stack's doze.hcl does not load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigError>,
    /// doze.hcl is newer than what the stack runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<ConfigChanged>,
    pub vitals: Vitals4,
    pub groups: Vec<Group>,
    /// The instances up, and declared, tasks aside.
    pub awake: usize,
    pub total: usize,
    pub grid: GridView,
    /// How long the daemon has been up: `14m`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub up: String,
    /// Set for a stack that is not running: its rows are down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
}

/// A heading over part of the stack.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Group {
    pub title: &'static str,
    pub what: &'static str,
    pub count: usize,
    pub rows: Vec<Row>,
}

/// One instance as a row or a tile shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub kind: String,
    pub look: Look,
    pub engine: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mem: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cpu: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub conns: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub up: String,
    pub because: String,
    /// warn while dozing or waking, bad in trouble, dim otherwise.
    pub because_tone: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub addr: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub port: String,
    pub awake: bool,
    pub trouble: bool,
    /// Its depth in the dependency tree.
    pub depth: usize,
    /// The address as shown; `url` is what Open uses, empty when it is not http.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// The switch: up or on its way, crashing included.
    pub on: bool,
    /// `doze off` took it out: the switch is greyed and turning it on brings it back.
    #[serde(skip_serializing_if = "is_false")]
    pub off: bool,
    /// `enabled = false` in doze.hcl: no switch at all.
    #[serde(skip_serializing_if = "is_false")]
    pub paused: bool,
    pub pinned: bool,
    /// The daemon is not answering: the row is the last word heard, and its switch cannot act.
    #[serde(skip_serializing_if = "is_false")]
    pub frozen: bool,
    /// The first line of what it last said, while it is in trouble.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub err: String,
    /// A row of a stopped stack; `last` says Up as last time starts it.
    #[serde(skip_serializing_if = "is_false")]
    pub down: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub last: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub run: String,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // WHY: serde's skip_serializing_if passes a reference
fn is_false(b: &bool) -> bool {
    !*b
}

/// The stack's overview at `now`.
pub fn home(st: &StackView, now: SystemTime) -> Home {
    let snap = st.words();
    let by_name: HashMap<&str, &InstanceView> =
        st.instances.iter().map(|v| (v.name.as_str(), v)).collect();
    let rows = words::rows(&snap);
    let (_, depth) = words::tree_order(&rows);
    let frozen = st.running && st.link.conn == Conn::Down;
    let mut h = head(st, now);
    let mut ordered: Vec<(&InstanceView, &words::Instance)> = Vec::new();
    for g in words::Group::ALL {
        let mut grp = Group {
            title: g.title(),
            what: g.what(),
            ..Group::default()
        };
        for r in rows.iter().filter(|r| r.group == g) {
            let Some(v) = by_name.get(r.v.name.as_str()) else {
                continue;
            };
            ordered.push((v, r.v));
            let mut row = row(v, r.v, &snap, now);
            row.depth = depth.get(r.v.name.as_str()).copied().unwrap_or(0);
            row.frozen = frozen;
            // Waking for minutes with no word from it: Needs you says so; the row says the same, not "starting".
            if let Some(it) = crate::attention::stuck_waking(st, r.v, now) {
                row.because = it.detail;
            }
            if r.v.kind != words::Kind::Task {
                h.total += 1;
                h.awake += usize::from(row.awake);
            }
            grp.rows.push(row);
        }
        grp.count = grp.rows.len();
        if grp.count > 0 {
            h.groups.push(grp);
        }
    }
    h.vitals = vitals(st, &snap);
    h.grid = grid(&ordered, now);
    h
}

/// The head of a stack's page: who, where, how long up, and whether to believe it.
pub(crate) fn head(st: &StackView, now: SystemTime) -> Home {
    let home = words::home();
    let up = match (st.running, st.since) {
        (true, Some(s)) => words::ago(now.duration_since(s).unwrap_or_default()),
        _ => String::new(),
    };
    Home {
        key: st.key.dir().display().to_string(),
        stack: st.name.clone(),
        dir: words::tilde(&st.dir, home.as_deref()).into_owned(),
        running: st.running,
        banner: banner(st, now),
        up,
        ..Home::default()
    }
}

/// Whether what the page shows can be believed (Go `BannerOf`).
pub(crate) fn banner(st: &StackView, now: SystemTime) -> Banner {
    if !st.running {
        return Banner {
            ok: true,
            text: String::new(),
        };
    }
    let l = &st.link;
    // Only a metered stack with something running is expected to speak every second (the meters are of running
    // instances); a quiet one, or one with nothing up, is not stale: its socket is open.
    let serving = st
        .instances
        .iter()
        .any(|v| matches!(v.state.as_str(), "active" | "idle"));
    let since = match (st.metered && serving, l.last_frame) {
        (true, Some(t)) => now.duration_since(t).unwrap_or_default(),
        _ => std::time::Duration::ZERO,
    };
    let retry = l
        .retry_at
        .and_then(|r| r.duration_since(now).ok())
        .unwrap_or_default();
    match words::banner(
        l.conn == Conn::Connecting,
        l.conn == Conn::Down,
        retry,
        since,
    ) {
        None => Banner {
            ok: true,
            text: String::new(),
        },
        Some(text) => Banner { ok: false, text },
    }
}

/// One running instance's row.
fn row(v: &InstanceView, i: &words::Instance, snap: &words::Snapshot, now: SystemTime) -> Row {
    let reach = words::reach(i);
    let (address, url) = address_of(v, i);
    let mut row = Row {
        name: v.name.clone(),
        kind: v.kind.clone(),
        look: Look::of(words::look(i)),
        engine: engine(v),
        because: words::because(i, snap, now),
        because_tone: because_tone(i),
        addr: reach
            .as_ref()
            .map(|r| r.addr.to_string())
            .unwrap_or_default(),
        port: reach.and_then(|r| r.port).unwrap_or_default().to_owned(),
        awake: words::awake_now(i),
        trouble: words::troubled(i),
        address,
        url,
        on: words::switch_on(i),
        off: i.disabled && !i.paused,
        paused: i.paused,
        pinned: i.keep_awake,
        ..Row::default()
    };
    if row.awake {
        row.mem = words::mb(v.ram);
        row.cpu = format!("{:.1}", v.cpu);
        row.conns = words::connections(i).to_string();
        if let Some(s) = i.started_at {
            row.up = words::ago(now.duration_since(s).unwrap_or_default());
        }
    }
    if row.trouble && !v.last_error.is_empty() {
        row.err = words::first_line(&v.last_error).to_owned();
    }
    row
}

/// The Engine column: a module's engine and version, `process` for the rest, `task` for a task.
pub(crate) fn engine(v: &InstanceView) -> String {
    match v.kind.as_str() {
        "module" => {
            let version = v.version.strip_prefix("builtin").unwrap_or(&v.version);
            format!("{} {version}", v.engine).trim().to_owned()
        }
        "task" => "task".to_owned(),
        _ if !v.engine.is_empty() => v.engine.clone(),
        _ => "process".to_owned(),
    }
}

fn because_tone(i: &words::Instance) -> &'static str {
    if words::troubled(i) {
        "bad"
    } else if matches!(i.state, words::State::Idle | words::State::Booting) {
        "warn"
    } else {
        "dim"
    }
}

/// A row's address as the overview shows it, and the URL Open uses (Go `addressOf`): an app's https domain, else the
/// port-less http name, else a module's name and port. A task has none.
pub(crate) fn address_of(v: &InstanceView, i: &words::Instance) -> (String, String) {
    if v.kind == "task" {
        return (String::new(), String::new());
    }
    if !v.https.is_empty() {
        return (v.https.clone(), v.https.clone());
    }
    if v.resource.starts_with("http://") || v.resource.starts_with("https://") {
        return (v.resource.clone(), v.resource.clone());
    }
    let Some(r) = words::reach(i) else {
        return (String::new(), String::new());
    };
    let whole = format!("{}{}", r.addr, r.port.unwrap_or_default());
    if r.addr.starts_with("http") && v.kind == "module" && v.url.is_empty() && v.resource.is_empty()
    {
        // A module is web only when it says so; its name alone is not.
        let a = words::address(i)
            .map(|a| a.into_owned())
            .unwrap_or_default();
        return (a, String::new());
    }
    if r.addr.starts_with("http") {
        return (whole.clone(), whole);
    }
    (whole, String::new())
}
