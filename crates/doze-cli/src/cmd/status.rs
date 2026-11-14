//! `doze status` (aliases `tree`, `ls`, `ps`): every service, its state, where to reach it and what it uses, as Go's
//! table draws it (cmd/doze/tree_render.go; `tests/transcripts/status*.txt` hold the Go goldens). One model,
//! [`Report`], drawn as the grouped table, as the dependency tree (`--graph`) and as Go's `status --json` shape.
//!
//! With the daemon up everything comes from its snapshot: each declared instance with its kind, needs and endpoint.
//! With the daemon down, from doze.hcl's shape (read offline, [`crate::config`]): every declared service, asleep, at
//! the address it will answer on, as Go lists them.

use crate::cli::args::Status;
use crate::cmd::{declared, watch};
use crate::ctx::Ctx;
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::stack::{self, Stack};
use crate::table::Table;
use crate::view::View;
use doze_control::{InstanceView, Snapshot};
use serde::Serialize;
use std::time::{Duration, SystemTime};

/// `doze status`.
pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Status, _notices: &mut Notices) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    if a.watch {
        return watch::run(ctx, mode, &stack, a.graph);
    }
    let report = match stack.read()? {
        Some(c) => Report::of(&stack::snapshot(&c)?, ctx.now),
        None => {
            let cfg = crate::config::shape_of(ctx, mode, &stack.project)?;
            Report::declared(declared::views(&cfg), ctx.now)
        }
    };
    Ok(Done::ok(if a.graph {
        View::Graph(Box::new(report))
    } else {
        View::Status(Box::new(report))
    }))
}

/// The stack, as status draws it.
#[derive(Debug, Clone)]
pub struct Report {
    pub(crate) running: bool,
    /// The stack's idle time, for "sleeps in".
    pub(crate) idle: Duration,
    pub(crate) now: SystemTime,
    /// The declared instances, in the daemon's order.
    pub(crate) views: Vec<InstanceView>,
}

impl Report {
    pub(crate) fn of(snap: &Snapshot, now: SystemTime) -> Report {
        Report {
            running: true,
            idle: snap.idle_timeout.to_std(),
            now,
            views: go_order(stack::declared(snap).cloned().collect()),
        }
    }

    /// The stack as doze.hcl declares it, with no daemon running it.
    pub(crate) fn declared(views: Vec<InstanceView>, now: SystemTime) -> Report {
        Report {
            running: false,
            idle: Duration::ZERO,
            now,
            views,
        }
    }

    /// The grouped table and the line under it (Go `renderTable`).
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = String::new();
        if !self.running {
            if self.views.is_empty() {
                return format!("{}\n", p.dim("doze is not running"));
            }
            out.push_str(&p.dim("doze is not running — showing declared services"));
            out.push_str("\n\n");
        }
        let mut t = Table::new(&[
            "", "NAME", "ENGINE", "STATE", "ENDPOINT", "CONNS", "MEM", "CPU", "NOTE",
        ]);
        for (cat, label) in CATEGORIES {
            let mut rows: Vec<&InstanceView> =
                self.views.iter().filter(|v| category(v) == cat).collect();
            if rows.is_empty() {
                continue;
            }
            rows.sort_by(|a, b| a.name.cmp(&b.name));
            t.label(label);
            for v in rows {
                t.row(self.cells(v, p));
            }
        }
        out.push_str(&t.draw(p));
        out.push('\n');
        if self.running {
            out.push('\n');
            out.push_str(&p.dim(&self.summary(" · connect to any endpoint to wake it")));
            out.push('\n');
        }
        out
    }

    /// "4 awake · 81M resident", and what follows it.
    pub(crate) fn summary(&self, tail: &str) -> String {
        let awake: Vec<&InstanceView> = self.views.iter().filter(|v| awake(&v.state)).collect();
        let ram: i64 = awake.iter().map(|v| v.ram).sum();
        match human_bytes(ram) {
            // Go prints "0 awake ·  resident" here; with nothing resident there is nothing to say about memory.
            b if b.is_empty() => format!("{} awake{tail}", awake.len()),
            b => format!("{} awake · {b} resident{tail}", awake.len()),
        }
    }

    fn cells(&self, v: &InstanceView, p: Paint) -> Vec<String> {
        let up = awake(&v.state);
        let dash = || p.dim("-").into_owned();
        let note = doze_display::note(&doze_control::words::instance(v), self.idle, self.now);
        vec![
            glyph(v, p),
            p.bold(&v.name).into_owned(),
            p.dim(&engine_cell(v)).into_owned(),
            state_cell(v, p),
            endpoint_text(v).unwrap_or_else(dash),
            if up && v.conns > 0 {
                format!("{}c", v.conns)
            } else {
                dash()
            },
            if v.ram > 0 {
                human_bytes(v.ram)
            } else {
                dash()
            },
            if up && v.cpu >= 0.5 {
                format!("{:.0}%", v.cpu)
            } else {
                dash()
            },
            if note.bad {
                p.fail(&note.text)
            } else {
                p.dim(&note.text)
            }
            .into_owned(),
        ]
    }

    /// Go's `status --json` result.
    pub(crate) fn json(&self) -> Json<'_> {
        Json {
            daemon: Daemon {
                running: self.running,
            },
            services: self
                .views
                .iter()
                .map(|v| service(v, self.running))
                .collect(),
        }
    }
}

/// The groups, in the order the table shows them.
const CATEGORIES: [(Category, &str); 4] = [
    (Category::Module, "Modules"),
    (Category::Service, "Services"),
    (Category::App, "Apps"),
    (Category::Task, "Tasks"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Module,
    Service,
    App,
    Task,
}

/// A `service` is a service, a `task` a task, an `app` (the process engine) an app, everything else a module.
fn category(v: &InstanceView) -> Category {
    match v.kind.as_str() {
        "service" => Category::Service,
        "task" => Category::Task,
        _ if v.kind == "app" || v.engine == "process" => Category::App,
        _ => Category::Module,
    }
}

/// Up, booting or dozing: a process is there.
pub(crate) fn awake(state: &str) -> bool {
    doze_display::State::parse(state).awake()
}

/// The state's glyph, coloured as its word.
pub(crate) fn glyph(v: &InstanceView, p: Paint) -> String {
    let (g, word) = match () {
        () if v.paused => ("⊘", "disabled"),
        () if v.state == "crashing" => ("↻", "crashing"),
        () if v.state == "failed" => ("✕", "failed"),
        () if v.tainted || v.state == "tainted" => ("!", "tainted"),
        () if v.state == "error" => ("✕", "error"),
        () if awake(&v.state) => match doze_display::State::parse(&v.state).word() {
            "dozing" => ("○", "dozing"),
            "waking" => ("◌", "waking"),
            _ => ("●", "awake"),
        },
        () => ("·", "asleep"),
    };
    paint_state(p, word, g)
}

/// The state word (Go `stateText`): "awake", "dozing", "crashing 2/5", "disabled".
pub(crate) fn state_cell(v: &InstanceView, p: Paint) -> String {
    match () {
        () if v.paused => paint_state(p, "disabled", "disabled"),
        () if v.state == "crashing" => format!(
            "{} {}/{}",
            paint_state(p, "crashing", "crashing"),
            v.restart_count,
            v.restart_max
        ),
        () if v.state == "failed" => paint_state(p, "failed", "failed"),
        () if v.tainted || v.state == "tainted" || v.state == "error" => {
            let word = if v.state.is_empty() {
                "asleep"
            } else {
                &v.state
            };
            p.fail(word).into_owned()
        }
        () if awake(&v.state) => {
            let word = doze_display::State::parse(&v.state).word().to_owned();
            paint_state(p, &word, &word)
        }
        () => paint_state(p, "asleep", "asleep"),
    }
}

/// `text` in the colour of state `word`: awake green, dozing and crashing amber, waking cyan, trouble red, asleep dim.
fn paint_state(p: Paint, word: &str, text: &str) -> String {
    match word {
        "awake" | "done" => p.ok(text),
        "dozing" | "crashing" => p.warn(text),
        "waking" => p.cmd(text),
        "failed" | "error" | "tainted" => p.fail(text),
        _ => p.dim(text),
    }
    .into_owned()
}

/// What the engine column says: `service`, `task`, `app`, or the module and its version.
pub(crate) fn engine_cell(v: &InstanceView) -> String {
    match category(v) {
        Category::Service => "service".into(),
        Category::Task => "task".into(),
        Category::App => "app".into(),
        Category::Module if matches!(v.version.as_str(), "" | "builtin" | "0") => v.engine.clone(),
        Category::Module => format!("{} {}", v.engine, v.version),
    }
}

/// Where a client reaches it: its name in the zone with the port when it has one, else the address.
pub(crate) fn endpoint(v: &InstanceView) -> Option<&str> {
    (!v.endpoint.is_empty()).then_some(v.endpoint.as_str())
}

/// The endpoint as the table and the tree print it (with the zone's name in place of the loopback address).
pub(crate) fn endpoint_text(v: &InstanceView) -> Option<String> {
    let ep = endpoint(v)?;
    Some(match ep.split_once(':') {
        Some((_, port)) if !v.domain.is_empty() => format!("{}:{port}", v.domain),
        _ => ep.to_owned(),
    })
}

/// Bytes as Go's `ui.HumanBytes`: `512K`, `43M`, `1.5G`; empty for none.
pub(crate) fn human_bytes(b: i64) -> String {
    const K: i64 = 1024;
    match b {
        ..=0 => String::new(),
        b if b < K * K => format!("{}K", b / K),
        b if b < K * K * K => format!("{}M", b / (K * K)),
        #[allow(clippy::cast_precision_loss)] // WHY: a size to one decimal place
        b => format!("{:.1}G", b as f64 / (K * K * K) as f64),
    }
}

/// Go's `statusJSON`: field names are a published contract.
#[derive(Debug, Serialize)]
pub(crate) struct Json<'a> {
    daemon: Daemon,
    services: Vec<Service<'a>>,
}

#[derive(Debug, Serialize)]
struct Daemon {
    running: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Service<'a> {
    name: &'a str,
    engine: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    version: &'a str,
    group: &'static str,
    state: &'a str,
    enabled: bool,
    #[serde(skip_serializing_if = "str::is_empty")]
    endpoint: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    bind: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    domain: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    url: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    resource: &'a str,
    conns: i64,
    mem_bytes: i64,
    cpu_percent: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    depends_on: Vec<&'a str>,
    /// What the daemon knows of it; absent when no daemon runs the stack.
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<&'a InstanceView>,
}

fn service(v: &InstanceView, running: bool) -> Service<'_> {
    let mut deps: Vec<&str> = v.needs.iter().map(String::as_str).collect();
    deps.sort_unstable();
    Service {
        name: &v.name,
        engine: &v.engine,
        version: &v.version,
        group: match category(v) {
            Category::Module => "module",
            Category::Service => "service",
            Category::App => "app",
            Category::Task => "task",
        },
        state: plain_state(v),
        enabled: !v.paused,
        endpoint: &v.endpoint,
        bind: &v.bind,
        domain: &v.domain,
        url: &v.url,
        resource: &v.resource,
        conns: v.conns,
        mem_bytes: v.ram,
        cpu_percent: v.cpu,
        depends_on: deps,
        live: running.then_some(v),
    }
}

/// The state a script reads (Go `plainState`): the daemon's own words, `disabled`, or `asleep`.
fn plain_state(v: &InstanceView) -> &str {
    match v.state.as_str() {
        _ if v.paused => "disabled",
        "" if v.tainted => "asleep",
        s if v.tainted || matches!(s, "tainted" | "error") => s,
        s if awake(s) || matches!(s, "crashing" | "failed" | "done") => s,
        _ => "asleep",
    }
}

/// The order Go's `status --json` lists services in: doze.hcl's modules and apps, then its services, then its tasks.
/// Within each the daemon's order stands (the declaration order itself is doze.hcl's, read in C2).
pub(crate) fn go_order(mut views: Vec<InstanceView>) -> Vec<InstanceView> {
    views.sort_by_key(|v| match v.kind.as_str() {
        "service" => 1,
        "task" => 2,
        _ => 0,
    });
    views
}
