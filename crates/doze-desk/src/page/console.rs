//! A module's console tab as the page draws it (Go `view.ConsoleOf`): the tree of rows, columns and blocks the module
//! published, each block resolved to what it shows now for the rows selected. The page keeps only which row of each
//! table is selected; what a detail shows, which stream is followed ([`follow`]) and what each cell says are decided
//! here, as the dash decided them.

use super::Ink;
use super::resolve::Resolver;
use crate::console::ConsoleView;
use doze_control as wire;
use doze_display::{self as words, NodeKind};
use std::collections::HashMap;

/// The selected row of each table, by block id: its index among the rows as the module sent them, whatever order
/// the table is shown in.
pub type Selection = HashMap<String, usize>;

/// The row picked in each table, by block id: what names it (its first column's value, the resource an action on
/// it targets). A person picks a row, not a place: the module re-reads its tables every 2 s and may send them in
/// another order (postgres orders its sessions by state), so the pick is kept by name and turned into a
/// [`Selection`] for each read ([`selection`]); kept by place, "Terminate session" would go to whichever session
/// moved into it.
pub type Picked = HashMap<String, String>;

/// The selection the picks name in the tables as last read: each table's row of that name. A name no longer there
/// (the session ended) is not in it, so the table's first row is selected and shown as such.
pub fn selection(c: &ConsoleView, tab: &str, picked: &Picked) -> Selection {
    let mut sel = Selection::new();
    let (Some(spec), Some(w)) = (&c.spec, c.words.as_deref()) else {
        return sel;
    };
    let Some(body) = body_of(spec, Some(w), tab) else {
        return sel;
    };
    for t in wire_walk(&body).into_iter().filter(|t| t.kind == "table") {
        let id = block_id(t);
        let (Some(name), Some(rows), Some(p)) =
            (picked.get(&id), c.tables.get(&t.panel), w.panel(&t.panel))
        else {
            continue;
        };
        if let Some(i) = rows.iter().position(|r| r.cell(p.first_col()) == name) {
            sel.insert(id, i);
        }
    }
    sel
}

/// How each table is shown, by block id: the column it is ordered by and the words it is filtered to.
pub type Views = HashMap<String, TableView>;

/// How one table is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableView {
    /// The column it is ordered by (its index among the table's columns), and whether downwards.
    pub sort: Option<(usize, bool)>,
    /// Only the rows with a cell that holds these words (any case).
    pub filter: String,
}

impl TableView {
    /// A click on column `i`'s header: ordered by it upwards, then downwards, then as the module sent it.
    pub fn click(&mut self, i: usize) {
        self.sort = match self.sort {
            Some((c, false)) if c == i => Some((i, true)),
            Some((c, true)) if c == i => None,
            _ => Some((i, false)),
        };
    }
}

/// A table this long gets a filter box; a shorter one is read at a glance.
pub const FILTER_FROM: usize = 12;

/// One console tab, resolved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConsolePage {
    pub name: String,
    pub tab: String,
    pub body: Node,
    /// The instance's state as the module last said it; `awake` says the console is being read.
    pub state: String,
    pub awake: bool,
    pub err: String,
    /// The line beside the tabs: how the console is read.
    pub note: String,
    /// The keys bar: select, each action's key, next table.
    pub keys: Vec<(String, String)>,
    /// The spec has no such tab.
    pub missing: bool,
    /// The module has not described its console yet.
    pub reading: bool,
}

/// A container (`row`, `col`) or a block, with what the block shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Node {
    pub kind: NodeKind,
    pub id: String,
    pub title: String,
    pub weight: u32,
    /// Takes the room that is left; the rest are as tall as their content.
    pub flexible: bool,
    pub children: Vec<Node>,
    pub cols: Vec<Col>,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub stats: Vec<Stat>,
    /// A detail block's pairs; empty says nothing is selected.
    pub detail: Vec<(String, String)>,
    pub actions: Vec<Action>,
    /// A table's actions on its selected row (those whose kind is the table's), drawn at that row or in the detail
    /// that follows it; an actions block lists only the rest. A detail lists those of the table it follows.
    pub row_actions: Vec<Action>,
    /// An actions block in a tab with nothing left to list (every action went to a table's row, or there are none):
    /// it draws nothing.
    pub moved: bool,
    /// A table whose selected row a detail block shows (its row actions are drawn there).
    pub detailed: bool,
    /// A table's or stream's rows before the filter.
    pub total: usize,
    /// The words a table is filtered to.
    pub filter: String,
    pub text: String,
    pub values: Vec<f64>,
    pub last: String,
    /// A chart's scale, its least and greatest value written, and what its x runs over ("last 120 readings").
    pub low: String,
    pub high: String,
    pub span: String,
    /// Why a block cannot draw: `no data source x`.
    pub missing: String,
}

impl Node {
    /// Whether it or anything under it is a stream.
    pub fn has_stream(&self) -> bool {
        self.kind == NodeKind::Stream || self.children.iter().any(Node::has_stream)
    }

    /// It and every node under it, in order.
    pub fn walk(&self) -> Vec<&Node> {
        let mut out = vec![self];
        for c in &self.children {
            out.extend(c.walk());
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Col {
    pub key: String,
    pub title: String,
    pub right: bool,
    /// How wide it needs to be, in characters: its title's and its widest cell's.
    pub chars: usize,
    /// It takes the width the others leave: the widest column of words.
    pub grow: bool,
    /// The table is ordered by it: `Some(true)` downwards.
    pub sort: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// What names it: its first column's value.
    pub key: String,
    /// Its index among the rows as the module sent them (what [`Selection`] holds).
    pub at: usize,
    pub cells: Vec<Cell>,
    pub faded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    pub text: String,
    pub ink: Ink,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub label: String,
    pub value: String,
    pub ink: Ink,
    /// The limit it is counted against (an `of` column after it): `100` in `4 of 100`.
    pub of: String,
    /// How full it is against that limit, 0 to 1, when both are figures.
    pub fill: Option<f32>,
    /// Why it has no figure yet, in words, when it has none.
    pub note: String,
}

/// One of the module's actions: its key, and the row it would act on now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub key: char,
    pub destructive: bool,
    pub hint: String,
    pub resource: String,
}

impl Action {
    /// It asks before it runs: it takes input, or deletes something.
    pub fn asks(&self) -> bool {
        self.destructive || !self.hint.is_empty()
    }
}

/// The console tab `tab` of `name`, for the selection `sel`, as Go's dash resolved it: every block where the module
/// put it (what the Go goldens hold).
pub fn console_page(
    name: &str,
    tab: &str,
    c: Option<&ConsoleView>,
    sel: &Selection,
) -> ConsolePage {
    page(name, tab, c, sel, None)
}

/// The console tab as a person works in it: each table ordered and filtered as `views` says, and an action on a
/// table's kind drawn at its selected row (and in the detail that follows it) rather than in an actions block.
pub fn console_page_viewed(
    name: &str,
    tab: &str,
    c: Option<&ConsoleView>,
    sel: &Selection,
    views: &Views,
) -> ConsolePage {
    page(name, tab, c, sel, Some(views))
}

fn page(
    name: &str,
    tab: &str,
    c: Option<&ConsoleView>,
    sel: &Selection,
    views: Option<&Views>,
) -> ConsolePage {
    let mut out = ConsolePage {
        name: name.to_owned(),
        tab: tab.to_owned(),
        note: format!("re-read every 2s while awake · never wakes {name}"),
        body: Node {
            kind: NodeKind::Col,
            ..Node::default()
        },
        keys: vec![("tab".to_owned(), "next tab".to_owned())],
        ..ConsolePage::default()
    };
    let Some(c) = c else {
        out.reading = true;
        return out;
    };
    out.err.clone_from(&c.err);
    let Some(spec) = &c.spec else {
        out.reading = true;
        return out;
    };
    out.state.clone_from(&c.state);
    out.awake = words::State::parse(&c.state).awake();
    let Some(body) = body_of(spec, c.words.as_deref(), tab) else {
        out.missing = true;
        return out;
    };
    let r = Resolver::new(c, spec, tab, sel, &body);
    let r = match views {
        Some(v) => r.live(v),
        None => r,
    };
    out.body = r.node(&body);
    // The keys of this tab: select where it has a table, and the actions it offers (an actions block's, or a table
    // row's), not every action the module has.
    let blocks = out.body.walk();
    out.keys = Vec::new();
    if blocks.iter().any(|n| n.kind == NodeKind::Table) {
        out.keys.push(("↑↓".to_owned(), "select".to_owned()));
    }
    let offered: Vec<&str> = blocks
        .iter()
        .flat_map(|n| n.actions.iter().chain(&n.row_actions))
        .map(|a| a.id.as_str())
        .collect();
    for a in c
        .actions
        .iter()
        .filter(|a| offered.contains(&a.id.as_str()))
    {
        let key = words::action_key(&a.id)
            .map(String::from)
            .unwrap_or_default();
        out.keys.push((key, a.label.to_lowercase()));
    }
    out.keys.push(("tab".to_owned(), "next tab".to_owned()));
    out
}

/// The module's blocks for the overview, resolved. The overview has no tab of its own in the spec, so a detail or
/// stream in one draws from the first row.
pub fn extensions(c: &ConsoleView) -> Vec<(String, Node)> {
    let Some(spec) = &c.spec else {
        return Vec::new();
    };
    let sel = Selection::new();
    spec.extend
        .iter()
        .map(|e| {
            let r = Resolver::new(c, spec, words::TAB_OVERVIEW, &sel, &e.node);
            (e.after.clone(), r.node(&e.node))
        })
        .collect()
}

/// The stream a tab follows and the row of its table it is for, from the selection, as the dash's `followStream`
/// decided: what the hub's console subscription asks for. Empty when the tab has no stream.
pub fn follow(c: &ConsoleView, tab: &str, sel: &Selection) -> (String, String) {
    let Some(spec) = &c.spec else {
        return Default::default();
    };
    let Some(body) = body_of(spec, c.words.as_deref(), tab) else {
        return Default::default();
    };
    let nodes = wire_walk(&body);
    let Some(stream) = nodes.iter().find(|n| n.kind == "stream") else {
        return Default::default();
    };
    let Some(p) = spec.panels.iter().find(|p| p.id == stream.panel) else {
        return Default::default();
    };
    let from = if stream.from.is_empty() {
        &p.select
    } else {
        &stream.from
    };
    let r = Resolver::new(c, spec, tab, sel, &body);
    let sector = nodes
        .iter()
        .find(|n| !from.is_empty() && n.kind == "table" && (&n.id == from || &n.panel == from))
        .map(|t| r.selected_key(&block_id(t), &t.panel))
        .unwrap_or_default();
    (stream.panel.clone(), sector)
}

/// The tab's layout as the module published it, or the default one for a spec of panels with no tabs.
fn body_of(
    spec: &wire::ConsoleSpec,
    w: Option<&words::ConsoleSpec>,
    tab: &str,
) -> Option<wire::ConsoleNode> {
    let id = tab.strip_prefix("console:");
    if let Some(t) = spec.tabs.iter().find(|t| id == Some(t.id.as_str())) {
        return Some(t.body.clone());
    }
    words::console_body(w, tab).map(|n| to_wire(&n))
}

fn to_wire(n: &words::Node) -> wire::ConsoleNode {
    wire::ConsoleNode {
        kind: n.kind.as_str().to_owned(),
        id: n.id.clone(),
        title: n.title.clone(),
        weight: i64::from(n.weight),
        panel: n.panel.clone(),
        from: n.from.clone(),
        children: n.children.iter().map(to_wire).collect(),
        ..wire::ConsoleNode::default()
    }
}

pub(super) fn wire_walk(n: &wire::ConsoleNode) -> Vec<&wire::ConsoleNode> {
    let mut out = vec![n];
    for c in &n.children {
        out.extend(wire_walk(c));
    }
    out
}

/// A block's name: its id, else its kind and panel. (Go `BlockID`.)
pub(super) fn block_id(n: &wire::ConsoleNode) -> String {
    if n.id.is_empty() {
        format!("{}:{}", n.kind, n.panel)
    } else {
        n.id.clone()
    }
}
