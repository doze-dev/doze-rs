//! The Grid (Go `view/grid.go`, the MapGrid board): the stack as a table that reads the same at thirty services as
//! at five. A row needs each column it has a dot in. The rows are the apps in the Map's order, callers first; the
//! columns are the apps something needs, then what the stack shares, most used first, then the tasks that run before
//! each app. The order is the Map's own (`doze-map`), so the two views never disagree about who comes first.

use std::collections::HashMap;
use std::time::SystemTime;

use doze_control::InstanceView;
use doze_display as words;
use serde::Serialize;

use super::and_list;
use super::refs::Refs;

/// The modules an app consumes events from; the Grid says what it cannot show for them yet.
const EVENT_ENGINES: [&str; 6] = ["kafka", "redpanda", "nats", "rabbitmq", "sqs", "pulsar"];

/// The stack as the Grid draws it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridView {
    /// The groups over the columns, each spanning its columns.
    pub heads: Vec<GridHead>,
    pub cols: Vec<GridCol>,
    pub rows: Vec<GridRow>,
    /// "needed by": a count per column.
    pub foot: Vec<String>,
    /// What the grid cannot show yet, when the stack has a broker.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub later: String,
}

/// A group's caption over its columns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridHead {
    pub text: &'static str,
    pub span: usize,
}

/// A column: an app, a shared service, or the tasks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridCol {
    pub name: String,
    /// `app`, `shared` or `tasks`.
    pub kind: &'static str,
    /// Starts a group: a line on its left.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sep: bool,
    /// The tasks column's tally: `9 ✓`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub sub: String,
}

/// An app and what it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridRow {
    pub name: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub lang: &'static str,
    pub cells: Vec<GridCell>,
}

/// One row against one column.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridCell {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dot: bool,
    /// The row's own column.
    #[serde(rename = "self", skip_serializing_if = "std::ops::Not::not")]
    pub own: bool,
    /// The tasks column's word: `migrate-orders ✓`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub tone: &'static str,
    /// What hovering or focusing the cell says; `None` for an empty one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<GridTip>,
}

/// The tooltip: what the dot means, why it matters, the line of doze.hcl that makes it so, what the keys do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GridTip {
    pub head: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// The reference as doze.hcl writes it (`CATALOG_URL = app.catalog.url`), or the reference alone when the file
    /// was not read.
    #[serde(rename = "ref")]
    pub reference: String,
    pub keys: String,
    /// What ⏎ opens: the column's service, or the first task.
    pub open: String,
    /// The row and the column, for [`GridView::refer`].
    #[serde(skip)]
    pub from: String,
    #[serde(skip)]
    pub to: String,
}

type Pair<'a> = (&'a InstanceView, &'a words::Instance);
type Links<'a> = HashMap<&'a str, Vec<&'a str>>;

/// The stack read as calls, uses and tasks (Go `readFlow`), by name.
#[derive(Default)]
struct Flow<'a> {
    by_name: HashMap<&'a str, Pair<'a>>,
    callers: Links<'a>,
    calls_and_uses: Links<'a>,
    users: Links<'a>,
    tasks: Links<'a>,
    /// Modules and services, in display order.
    shared: Vec<&'a str>,
}

fn is_shared(kind: &str) -> bool {
    matches!(kind, "module" | "service" | "")
}

fn get<'m, 'a>(m: &'m Links<'a>, k: &str) -> &'m [&'a str] {
    m.get(k).map_or(&[], Vec::as_slice)
}

impl<'a> Flow<'a> {
    fn read(ins: &[Pair<'a>]) -> Flow<'a> {
        let mut f = Flow::default();
        for &(v, i) in ins {
            f.by_name.entry(v.name.as_str()).or_insert((v, i));
            if is_shared(&v.kind) {
                f.shared.push(&v.name);
            }
        }
        for &(v, _) in ins.iter().filter(|(v, _)| v.kind == "app") {
            let a = v.name.as_str();
            let mut seen: Vec<&str> = Vec::new();
            for need in &v.needs {
                let Some(&(d, _)) = f.by_name.get(need.as_str()) else {
                    continue;
                };
                let d_name = d.name.as_str();
                if d_name == a || seen.contains(&d_name) {
                    continue;
                }
                seen.push(d_name);
                if d.kind == "app" || is_shared(&d.kind) {
                    f.calls_and_uses.entry(a).or_default().push(d_name);
                }
                if d.kind == "app" {
                    f.callers.entry(d_name).or_default().push(a);
                } else if is_shared(&d.kind) {
                    f.users.entry(d_name).or_default().push(a);
                } else {
                    f.tasks.entry(a).or_default().push(d_name);
                }
            }
        }
        f
    }

    fn kind(&self, name: &str) -> &'a str {
        self.by_name.get(name).map_or("", |(v, _)| v.kind.as_str())
    }

    fn awake(&self, name: &str) -> bool {
        self.by_name
            .get(name)
            .is_some_and(|(_, i)| words::awake_now(i))
    }
}

/// The apps in the Map's order: down each column of calls, then the lane.
fn map_order(ins: &[Pair<'_>]) -> Vec<String> {
    // Names as a person reads them (`api-2`, `api-7`, `api-26`), as the List and the sidebar order them.
    let mut input: Vec<doze_map::Instance> = ins.iter().map(|(v, _)| map_instance(v)).collect();
    input.sort_by(|a, b| words::natural_cmp(&a.name, &b.name));
    let layout = doze_map::layout(&input, &doze_map::Options::default());
    layout
        .cards
        .iter()
        .filter(|c| {
            c.kind == doze_map::Kind::App
                && matches!(c.role, doze_map::Role::Flow | doze_map::Role::Lane)
        })
        .map(|c| c.name.clone())
        .collect()
}

/// An instance as the Map lays it out.
pub(crate) fn map_instance(v: &InstanceView) -> doze_map::Instance {
    let kind = match v.kind.as_str() {
        "app" => doze_map::Kind::App,
        "task" => doze_map::Kind::Task,
        "service" => doze_map::Kind::Service,
        _ => doze_map::Kind::Module,
    };
    let mut i = doze_map::Instance::new(&v.name, kind, &[]);
    i.needs.clone_from(&v.needs);
    i.run.clone_from(&v.run);
    i.engine = v.engine.split_whitespace().next().unwrap_or("").to_owned();
    i.busy = kind == doze_map::Kind::Task
        && (v.state == "booting" || v.task.as_ref().is_some_and(|t| t.result == "failed"));
    if !v.group.is_empty() {
        i.group = Some(v.group.clone());
    }
    i
}

/// The Grid of a stack whose instances are `ins`, in display order, at `now`.
pub fn grid(ins: &[Pair<'_>], now: SystemTime) -> GridView {
    let f = Flow::read(ins);
    let order = map_order(ins);
    let mut g = GridView::default();
    let apps: Vec<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|a| !get(&f.callers, a).is_empty())
        .collect();
    let mut shared: Vec<&str> = f
        .shared
        .iter()
        .copied()
        .filter(|s| !get(&f.users, s).is_empty())
        .collect();
    shared.sort_by(|a, b| {
        get(&f.users, b)
            .len()
            .cmp(&get(&f.users, a).len())
            .then_with(|| words::natural_cmp(a, b))
    });
    columns(&mut g, &f, &apps, &shared, &order);
    let mut count = vec![0usize; g.cols.len()];
    for a in &order {
        let row = row(&f, &g.cols, a, now, &mut count);
        g.rows.push(row);
    }
    g.foot = g
        .cols
        .iter()
        .zip(&count)
        .map(|(c, n)| {
            if c.kind == "tasks" {
                String::new()
            } else {
                n.to_string()
            }
        })
        .collect();
    g.later = later(&f, &g.cols);
    g
}

/// The heads and the columns.
fn columns(g: &mut GridView, f: &Flow<'_>, apps: &[&str], shared: &[&str], order: &[String]) {
    if !apps.is_empty() {
        g.heads.push(GridHead {
            text: "Other services",
            span: apps.len(),
        });
        g.cols.extend(apps.iter().map(|a| GridCol {
            name: (*a).to_owned(),
            kind: "app",
            ..GridCol::default()
        }));
    }
    if !shared.is_empty() {
        g.heads.push(GridHead {
            text: "Shared",
            span: shared.len(),
        });
        g.cols
            .extend(shared.iter().enumerate().map(|(i, s)| GridCol {
                name: (*s).to_owned(),
                kind: "shared",
                sep: i == 0 && !apps.is_empty(),
                ..GridCol::default()
            }));
    }
    if order.iter().any(|a| !get(&f.tasks, a).is_empty()) {
        let mut seen: Vec<&str> = Vec::new();
        let mut done = 0;
        for t in order.iter().flat_map(|a| get(&f.tasks, a)) {
            if seen.contains(t) {
                continue;
            }
            seen.push(t);
            if let Some((v, _)) = f.by_name.get(t)
                && v.task.as_ref().is_some_and(|r| r.result == "done")
                && v.state != "booting"
            {
                done += 1;
            }
        }
        g.heads.push(GridHead {
            text: "Before it",
            span: 1,
        });
        g.cols.push(GridCol {
            name: "tasks".to_owned(),
            kind: "tasks",
            sep: !g.cols.is_empty(),
            sub: format!("{done} ✓"),
        });
    }
}

/// One app's row.
fn row(f: &Flow<'_>, cols: &[GridCol], a: &str, now: SystemTime, count: &mut [usize]) -> GridRow {
    let run = f.by_name.get(a).map_or("", |(v, _)| v.run.as_str());
    let needs = get(&f.calls_and_uses, a);
    let mut row = GridRow {
        name: a.to_owned(),
        lang: doze_map::Lang::of(run).map_or("", doze_map::Lang::badge),
        cells: Vec::with_capacity(cols.len()),
    };
    for (ci, c) in cols.iter().enumerate() {
        let mut cell = GridCell::default();
        if c.kind == "tasks" {
            let ts = get(&f.tasks, a);
            if let Some(first) = ts.first() {
                (cell.text, cell.tone) = task_cell(f, ts);
                let lines: Vec<String> = ts.iter().map(|t| task_line(f, t, now)).collect();
                let refs: Vec<String> = ts.iter().map(|t| format!("task.{t}")).collect();
                cell.tip = Some(GridTip {
                    head: format!("{} run before {a}", and_list(ts)),
                    why: lines.join(" · "),
                    reference: format!("after = [{}]", refs.join(", ")),
                    keys: format!("⏎ opens {first} · o opens {a}"),
                    open: (*first).to_owned(),
                    from: a.to_owned(),
                    to: "after".to_owned(),
                });
            }
        } else if c.name == a {
            cell.own = true;
        } else if needs.contains(&c.name.as_str()) {
            cell.dot = true;
            if let Some(n) = count.get_mut(ci) {
                *n += 1;
            }
            cell.tip = Some(need_tip(f, c, a));
        }
        row.cells.push(cell);
    }
    row
}

fn need_tip(f: &Flow<'_>, c: &GridCol, a: &str) -> GridTip {
    let (verb, kind) = if c.kind == "shared" {
        let k = f.kind(&c.name);
        ("uses", if k.is_empty() { "module" } else { k })
    } else {
        ("needs", "app")
    };
    GridTip {
        head: format!("{a} {verb} {}", c.name),
        why: if f.awake(a) && f.awake(&c.name) {
            "keeps it awake".to_owned()
        } else {
            String::new()
        },
        reference: format!("{kind}.{}", c.name),
        keys: format!("⏎ opens {} · o opens {a}", c.name),
        open: c.name.clone(),
        from: a.to_owned(),
        to: c.name.clone(),
    }
}

/// The tasks column's word for an app: the task when it is one, how many when more, marked as the last runs went.
fn task_cell(f: &Flow<'_>, ts: &[&str]) -> (String, &'static str) {
    let (mut word, mut tone) = ("✓", "ok");
    for t in ts {
        let Some((v, _)) = f.by_name.get(t) else {
            continue;
        };
        if v.state == "booting" {
            if tone != "bad" {
                (word, tone) = (words::Look::Running.word(), "cool");
            }
        } else if v.task.is_none() {
            if tone == "ok" {
                (word, tone) = (words::Look::NotRun.word(), "dim");
            }
        } else if v.task.as_ref().is_some_and(|r| r.result == "failed") {
            (word, tone) = ("✗", "bad");
        }
    }
    let name = match ts {
        [one] => (*one).to_owned(),
        _ => format!("{} tasks", ts.len()),
    };
    (format!("{name} {word}"), tone)
}

/// How one task last went: `migrate ✓ 18m ago`.
fn task_line(f: &Flow<'_>, t: &str, now: SystemTime) -> String {
    let Some((v, i)) = f.by_name.get(t) else {
        return t.to_owned();
    };
    let ago = || {
        let ran = i.task.as_ref().and_then(|r| r.ran_at).unwrap_or(now);
        words::ago(now.duration_since(ran).unwrap_or_default())
    };
    match &v.task {
        _ if v.state == "booting" => format!("{t} running"),
        None => format!("{t} not run yet"),
        Some(r) if r.result == "failed" => format!("{t} failed {} ago", ago()),
        Some(_) => format!("{t} ✓ {} ago", ago()),
    }
}

/// What the Grid cannot show yet, when the stack shares an event broker.
fn later(f: &Flow<'_>, cols: &[GridCol]) -> String {
    cols.iter()
        .filter(|c| c.kind == "shared")
        .find_map(|c| {
            let e = f.by_name.get(c.name.as_str()).map_or("", |(v, _)| v.engine.as_str());
            EVENT_ENGINES.contains(&e).then(|| {
                format!(
                    "Not drawn yet: which {e} topics each service writes and reads. doze sees that a service connects to {}, not what it publishes; that needs the module to tell doze which connection is which service.",
                    c.name
                )
            })
        })
        .unwrap_or_default()
}

impl GridView {
    /// Puts the file's own words in the tooltips (Go `Refer`).
    pub fn refer(&mut self, refs: &Refs) {
        for cell in self.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
            if let Some(t) = &mut cell.tip
                && let Some(s) = refs.get(&t.from).and_then(|m| m.get(&t.to))
                && !s.is_empty()
            {
                t.reference.clone_from(s);
            }
        }
    }

    /// The tooltip at a row and a column, if the cell has one.
    pub fn tip(&self, row: usize, col: usize) -> Option<&GridTip> {
        self.rows.get(row)?.cells.get(col)?.tip.as_ref()
    }
}
