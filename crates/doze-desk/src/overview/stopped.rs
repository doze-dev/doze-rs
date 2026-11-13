//! A stopped stack's overview (Go `view/stopped.go`): the same page in another state. The rows are the ones the
//! running stack shows, in the same groups and order, read from doze.hcl since there is no daemon to ask; each is
//! down, its switch off, ticked when Up as last time starts it.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

use doze_control::InstanceView;
use doze_display as words;
use serde::Serialize;

use super::config::{ConfigError, config_error};
use super::grid::grid;
use super::home::{Group, Home, Row, head};
use super::shape::{Decl, Shape};
use super::{Look, and_list};
use crate::convert;
use crate::view::StackView;

/// What the page adds for a stack that is not running.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stopped {
    /// `last up 3 days ago`, or `never up`.
    pub when: String,
    /// What Up as last time does: `Up starts api, web and db, as last time, and what they need.`
    pub last_line: String,
    /// What was started last time; empty means everything.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub last_set: Vec<String>,
    /// doze.hcl does not load, in one line.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub problem: String,
    /// That mistake read out: the file, the line, what to write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigError>,
}

/// The overview of the stopped stack `st`, whose doze.hcl reads as `shape`, at `now`.
pub fn stopped_home(st: &StackView, shape: &Shape, now: SystemTime) -> Home {
    let mut h = head(st, now);
    let mut out = stopped(st, now);
    if let Some(err) = &shape.error {
        let mut ce = config_error(&Path::new(&st.dir).join("doze.hcl"), err, None, now);
        ce.text.clear();
        out.problem = ce.problem();
        out.config = Some(ce);
        h.stopped = Some(out);
        return h;
    }
    down_groups(&mut h, st, shape, now);
    h.stopped = Some(out);
    h
}

/// When it was up, and what Up as last time does.
fn stopped(st: &StackView, now: SystemTime) -> Stopped {
    Stopped {
        when: match st.last_up {
            Some(t) => format!(
                "last up {} ago",
                words::ago(now.duration_since(t).unwrap_or_default())
            ),
            None => "never up".to_owned(),
        },
        last_line: if st.last_set.is_empty() {
            "Up starts everything, as last time.".to_owned()
        } else {
            format!(
                "Up starts {}, as last time, and what they need.",
                and_list(&st.last_set)
            )
        },
        last_set: st.last_set.clone(),
        ..Stopped::default()
    }
}

/// The rows, each down, under their groups, and the Grid of them.
fn down_groups(h: &mut Home, st: &StackView, shape: &Shape, now: SystemTime) {
    let stack = if shape.stack.is_empty() {
        st.name.as_str()
    } else {
        shape.stack.as_str()
    };
    let on = starts(&shape.decls, &st.last_set);
    let views: Vec<InstanceView> = shape.decls.iter().map(view_of).collect();
    let snap = convert::snapshot(st.idle_timeout, &views);
    let rows = words::rows(&snap);
    let (_, depth) = words::tree_order(&rows);
    let by_name: HashMap<&str, (&Decl, &InstanceView)> = shape
        .decls
        .iter()
        .zip(&views)
        .map(|(d, v)| (d.name.as_str(), (d, v)))
        .collect();
    let mut ordered = Vec::new();
    for g in words::Group::ALL {
        let mut grp = Group {
            title: g.title(),
            what: g.what(),
            ..Group::default()
        };
        for r in rows.iter().filter(|r| r.group == g) {
            let Some(&(d, v)) = by_name.get(r.v.name.as_str()) else {
                continue;
            };
            ordered.push((v, r.v));
            let mut row = down_row(d, &on, &dependants(&shape.decls, &d.name), stack);
            row.depth = depth.get(d.name.as_str()).copied().unwrap_or(0);
            if d.kind != "task" {
                h.total += 1;
            }
            grp.rows.push(row);
        }
        grp.count = grp.rows.len();
        if grp.count > 0 {
            h.groups.push(grp);
        }
    }
    h.grid = grid(&ordered, now);
    h.grid.refer(&shape.file.refs);
}

/// The instance as a daemon would list it, so the rows sort and the Map places exactly as they will once up.
pub(crate) fn view_of(d: &Decl) -> InstanceView {
    InstanceView {
        name: d.name.clone(),
        kind: d.kind.clone(),
        run: d.run.clone(),
        needs: d.needs.clone(),
        engine: if d.kind == "module" {
            d.engine.clone()
        } else {
            String::new()
        },
        version: if d.kind == "module" {
            d.version.clone()
        } else {
            String::new()
        },
        ..InstanceView::default()
    }
}

/// What Up starts: the apps of the last set (every app when nothing is remembered), and what they need, all the
/// way down; nothing turned off in the file.
fn starts(decls: &[Decl], last_set: &[String]) -> HashSet<String> {
    let by_name: HashMap<&str, &Decl> = decls.iter().map(|d| (d.name.as_str(), d)).collect();
    let mut on: HashSet<String> = HashSet::new();
    let mut todo: Vec<&str> = decls
        .iter()
        .filter(|d| d.kind != "task" && d.enabled)
        .filter(|d| last_set.is_empty() || last_set.contains(&d.name))
        .map(|d| d.name.as_str())
        .collect();
    while let Some(name) = todo.pop() {
        let Some(d) = by_name.get(name).filter(|d| d.enabled) else {
            continue;
        };
        if on.insert(name.to_owned()) {
            todo.extend(d.needs.iter().map(String::as_str));
        }
    }
    on
}

/// Who refers to `name`, in the file's order.
fn dependants<'a>(decls: &'a [Decl], name: &str) -> Vec<&'a str> {
    decls
        .iter()
        .filter(|d| d.needs.iter().any(|n| n == name))
        .map(|d| d.name.as_str())
        .collect()
}

/// One declared instance of a stopped stack: down, with what it needs and what it is to the others.
fn down_row(d: &Decl, on: &HashSet<String>, users: &[&str], stack: &str) -> Row {
    let mut row = Row {
        name: d.name.clone(),
        kind: d.kind.clone(),
        engine: match d.kind.as_str() {
            "module" => format!("{} {}", d.engine, d.version).trim().to_owned(),
            "task" => "task".to_owned(),
            _ => "process".to_owned(),
        },
        down: true,
        because_tone: "dim",
        look: Look::STOPPED,
        needs: d.needs.clone(),
        run: d.run.clone(),
        address: d.address_when_up(stack),
        last: on.contains(&d.name) && d.kind != "task",
        ..Row::default()
    };
    row.because = match (d.kind.as_str(), users) {
        _ if !d.enabled => {
            row.paused = true;
            row.look = Look::PAUSED;
            "enabled = false in doze.hcl".to_owned()
        }
        ("task", _) if !d.needs.is_empty() => {
            format!("needs {} · runs once the stack is up", and_list(&d.needs))
        }
        ("task", _) => "runs once the stack is up".to_owned(),
        ("module", [one]) => format!("{one} uses it"),
        ("module", []) => "nothing uses it yet".to_owned(),
        ("module", many) => format!("{} use it", and_list(many)),
        _ if !d.needs.is_empty() => format!("needs {}", and_list(&d.needs)),
        _ => "needs nothing".to_owned(),
    };
    row
}
