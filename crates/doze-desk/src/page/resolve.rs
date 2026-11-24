//! What each block of a console tab shows now: the resolver behind [`super::console`] (Go `view.resolver`).

use super::console::{
    Action, Cell, Col, Node, Row, Selection, Views, block_id, row_ident, wire_walk,
};
use super::shape::{shown, size_columns, stats};
use crate::console::ConsoleView;
use crate::convert;
use doze_control as wire;
use doze_display::{self as words, NodeKind};
use std::collections::HashMap;

/// What resolves the blocks of one tab.
pub(super) struct Resolver<'a> {
    c: &'a ConsoleView,
    spec: &'a wire::ConsoleSpec,
    words: Option<&'a words::ConsoleSpec>,
    tab: &'a str,
    sel: &'a Selection,
    body: &'a wire::ConsoleNode,
    tables: HashMap<String, Vec<words::ConsoleRow>>,
    /// A tab a person works in (not the overview's blocks): tables are ordered and filtered as `views` says, and an
    /// action on a table's kind goes to its selected row.
    views: Option<&'a Views>,
}

impl<'a> Resolver<'a> {
    pub(super) fn new(
        c: &'a ConsoleView,
        spec: &'a wire::ConsoleSpec,
        tab: &'a str,
        sel: &'a Selection,
        body: &'a wire::ConsoleNode,
    ) -> Resolver<'a> {
        Resolver {
            c,
            spec,
            words: c.words.as_deref(),
            tab,
            sel,
            body,
            tables: c
                .tables
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            views: None,
        }
    }

    /// For a tab a person works in, ordered and filtered as `views` says.
    pub(super) fn live(mut self, views: &'a Views) -> Self {
        self.views = Some(views);
        self
    }

    fn panel(&self, id: &str) -> Option<(&'a wire::ConsolePanel, &'a words::Panel)> {
        Some((
            self.spec.panels.iter().find(|p| p.id == id)?,
            self.words?.panel(id)?,
        ))
    }

    pub(super) fn node(&self, n: &wire::ConsoleNode) -> Node {
        let w = convert::node(n);
        let mut out = Node {
            kind: w.kind.clone(),
            id: block_id(n),
            title: n.title.clone(),
            weight: w.share(),
            flexible: w.flexible(),
            ..Node::default()
        };
        match &w.kind {
            NodeKind::Row | NodeKind::Col => {
                out.children = n.children.iter().map(|c| self.node(c)).collect();
            }
            NodeKind::Table => self.table(&mut out, n),
            NodeKind::Stat => self.stat(&mut out, n),
            NodeKind::Detail => {
                default_title(&mut out, "detail");
                out.detail = words::detail_rows(self.words, self.tab, &self.tables, self.sel, &w)
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.into_owned()))
                    .collect();
                out.row_actions = self.detail_actions(n);
            }
            NodeKind::Stream => self.stream(&mut out, n),
            NodeKind::Actions => {
                default_title(&mut out, "actions");
                out.actions = self.actions(n);
                // In a tab, an actions block with nothing left to list (all went to rows, or the module has none) goes.
                out.moved = out.actions.is_empty() && self.views.is_some();
            }
            NodeKind::Note => {
                default_title(&mut out, "note");
                out.text.clone_from(&n.text);
            }
            NodeKind::Chart => self.chart(&mut out, n),
            // A block this doze cannot draw says so where it would be, rather than leave a hole in the layout.
            NodeKind::Other(kind) if kind.is_empty() => {
                out.missing =
                    "This part of the console has no kind; the module may be newer than this doze."
                        .to_owned();
            }
            NodeKind::Other(kind) => {
                out.missing = format!("This needs a newer doze: the module draws a “{kind}” here.");
            }
        }
        out
    }

    /// The selected row's identity ([`row_ident`]).
    fn selected_ident(&self, block: &str, panel: &str) -> String {
        let (Some(rows), Some((_, p))) = (self.c.tables.get(panel), self.panel(panel)) else {
            return String::new();
        };
        let i = self
            .sel
            .get(block)
            .copied()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
        row_ident(rows, i, p.first_col())
    }

    /// The selected row's first cell: what names it.
    pub(super) fn selected_key(&self, block: &str, panel: &str) -> String {
        let (Some(rows), Some((_, p))) = (self.c.tables.get(panel), self.panel(panel)) else {
            return String::new();
        };
        let i = self
            .sel
            .get(block)
            .copied()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
        rows.get(i)
            .map(|r| r.cell(p.first_col()).to_owned())
            .unwrap_or_default()
    }

    fn rows(
        &self,
        cols: &[(&wire::ConsoleColumn, &words::Column)],
        first: &str,
        rows: &[words::ConsoleRow],
    ) -> Vec<Row> {
        rows.iter()
            .enumerate()
            .map(|(at, row)| Row {
                key: row.cell(first).to_owned(),
                ident: row_ident(rows, at, first),
                at,
                faded: row.tone == "dim",
                cells: cols
                    .iter()
                    .map(|(_, c)| {
                        let (text, tone) = words::cell(c, row);
                        Cell {
                            text: text.into_owned(),
                            ink: tone.into(),
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    fn table(&self, out: &mut Node, n: &wire::ConsoleNode) {
        let Some((wp, p)) = self.panel(&n.panel) else {
            out.missing = format!("no data source {}", n.panel);
            return;
        };
        default_title(out, &p.title);
        let rows = self
            .c
            .tables
            .get(&n.panel)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut cols: Vec<(&wire::ConsoleColumn, &words::Column)> = wp
            .columns
            .iter()
            .zip(&p.columns)
            .filter(|(wc, _)| wc.role != "detail")
            .collect();
        // Rows made while the stack ran, not in the config, say so: a column of their own when any row is one.
        let runtime = rows.iter().any(|r| !r.cell(words::CELL_RUNTIME).is_empty());
        if runtime && !cols.iter().any(|(c, _)| c.key == "from") {
            cols.push((&FROM_WIRE, &FROM));
        }
        out.cols = cols.iter().map(|(c, _)| col(c)).collect();
        out.rows = self.rows(&cols, p.first_col(), rows);
        out.total = out.rows.len();
        size_columns(&mut out.cols, &out.rows);
        if !rows.is_empty() {
            out.selected = self
                .sel
                .get(&out.id)
                .copied()
                .unwrap_or(0)
                .min(rows.len() - 1);
        }
        if let Some(view) = self.views.and_then(|v| v.get(&out.id)) {
            shown(out, &cols, rows, view);
        }
        if self.views.is_some() {
            let key = self.selected_key(&out.id, &n.panel);
            let row = self.selected_ident(&out.id, &n.panel);
            out.row_actions = self
                .c
                .actions
                .iter()
                .filter(|a| !a.kind.is_empty() && a.kind == wp.source)
                .map(|a| Action {
                    row: row.clone(),
                    ..action(a, key.clone())
                })
                .collect();
            out.detailed = wire_walk(self.body).iter().any(|d| {
                d.kind == "detail" && !d.from.is_empty() && (d.from == out.id || d.from == n.panel)
            });
        }
    }

    fn stat(&self, out: &mut Node, n: &wire::ConsoleNode) {
        let Some((_, p)) = self.panel(&n.panel) else {
            out.missing = format!("no data source {}", n.panel);
            return;
        };
        default_title(out, &p.title);
        let empty = words::ConsoleRow::default();
        let row = self
            .c
            .tables
            .get(&n.panel)
            .and_then(|r| r.first())
            .unwrap_or(&empty);
        out.stats = stats(&p.columns, row);
    }

    fn stream(&self, out: &mut Node, n: &wire::ConsoleNode) {
        let Some((wp, p)) = self.panel(&n.panel) else {
            out.missing = format!("no data source {}", n.panel);
            return;
        };
        default_title(out, &p.title);
        let cols: Vec<_> = wp.columns.iter().zip(&p.columns).collect();
        out.cols = cols.iter().map(|(c, _)| col(c)).collect();
        let rows = self
            .c
            .streams
            .get(&n.panel)
            .map(Vec::as_slice)
            .unwrap_or_default();
        out.rows = self.rows(&cols, p.first_col(), rows);
        out.total = out.rows.len();
        size_columns(&mut out.cols, &out.rows);
        // Empty, a stream is a line saying so; with lines, it takes the room left.
        out.flexible = !out.rows.is_empty();
    }

    /// The actions a block lists, each with the row it would act on: the selected row of the table whose source is
    /// its kind, else of the first table. (Go `resolver.actions`.)
    fn actions(&self, n: &wire::ConsoleNode) -> Vec<Action> {
        let tables: Vec<&wire::ConsoleNode> = wire_walk(self.body)
            .into_iter()
            .filter(|x| x.kind == "table")
            .collect();
        self.c
            .actions
            .iter()
            .filter(|a| n.actions.is_empty() || n.actions.contains(&a.id))
            .filter(|a| !self.on_a_row(a, &tables))
            .map(|a| {
                let mut res = String::new();
                for t in &tables {
                    if self
                        .spec
                        .panels
                        .iter()
                        .any(|p| p.id == t.panel && p.source == a.kind)
                    {
                        res = self.selected_key(&block_id(t), &t.panel);
                    }
                }
                // The first table's row, unless the action is on something another panel names (valkey's "Empty the
                // cache" is on the server its counters show, not on the database row Go's fallback picked).
                let elsewhere = self.spec.panels.iter().any(|p| p.source == a.kind);
                if let Some(t) = tables
                    .first()
                    .filter(|_| res.is_empty() && !a.kind.is_empty() && !elsewhere)
                {
                    res = self.selected_key(&block_id(t), &t.panel);
                }
                action(a, res)
            })
            .collect()
    }

    /// In a tab a person works in, an action on the kind a table of the tab lists is drawn at that table's selected
    /// row, not in an actions block.
    fn on_a_row(&self, a: &wire::ActionView, tables: &[&wire::ConsoleNode]) -> bool {
        self.views.is_some()
            && !a.kind.is_empty()
            && tables.iter().any(|t| {
                self.spec
                    .panels
                    .iter()
                    .any(|p| p.id == t.panel && p.source == a.kind)
            })
    }

    /// The actions of the table a detail follows, on the row it shows.
    fn detail_actions(&self, n: &wire::ConsoleNode) -> Vec<Action> {
        if self.views.is_none() || n.from.is_empty() {
            return Vec::new();
        }
        let Some(t) = wire_walk(self.body).into_iter().find(|t| {
            t.kind == "table" && (t.id == n.from || t.panel == n.from || block_id(t) == n.from)
        }) else {
            return Vec::new();
        };
        let Some(p) = self.spec.panels.iter().find(|p| p.id == t.panel) else {
            return Vec::new();
        };
        let key = self.selected_key(&block_id(t), &t.panel);
        let row = self.selected_ident(&block_id(t), &t.panel);
        self.c
            .actions
            .iter()
            .filter(|a| !a.kind.is_empty() && a.kind == p.source)
            .map(|a| Action {
                row: row.clone(),
                ..action(a, key.clone())
            })
            .collect()
    }

    fn chart(&self, out: &mut Node, n: &wire::ConsoleNode) {
        let Some((_, p)) = self.panel(&n.panel) else {
            out.missing = format!("no data source {}", n.panel);
            return;
        };
        default_title(out, "chart");
        let first = p.first_col();
        for row in self
            .c
            .tables
            .get(&n.panel)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            for c in p.columns.iter().filter(|c| c.key != first) {
                if let Some(f) = words::parse_float(&row.cell(&c.key).replace(',', "")) {
                    out.values.push(f);
                }
            }
        }
        out.last = out
            .values
            .last()
            .map(|&v| words::count(v))
            .unwrap_or_default();
        if out.values.len() > 1 {
            let (lo, hi) = out
                .values
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| {
                    (l.min(v), h.max(v))
                });
            out.low = words::count(lo);
            out.high = words::count(hi);
            out.span = format!("last {} readings", out.values.len());
        }
    }
}

static FROM_WIRE: std::sync::LazyLock<wire::ConsoleColumn> =
    std::sync::LazyLock::new(|| wire::ConsoleColumn {
        key: "from".to_owned(),
        title: "from".to_owned(),
        ..wire::ConsoleColumn::default()
    });
static FROM: std::sync::LazyLock<words::Column> = std::sync::LazyLock::new(|| words::Column {
    key: "from".to_owned(),
    title: "from".to_owned(),
    role: words::Role::Text,
});

fn col(c: &wire::ConsoleColumn) -> Col {
    Col {
        key: c.key.clone(),
        title: c.title.clone(),
        right: c.align == "right",
        ..Col::default()
    }
}

fn action(a: &wire::ActionView, resource: String) -> Action {
    Action {
        id: a.id.clone(),
        label: a.label.clone(),
        key: words::action_key(&a.id).unwrap_or('?'),
        destructive: a.destructive,
        hint: a.input_hint.clone(),
        resource,
        row: String::new(),
    }
}

fn default_title(out: &mut Node, title: &str) {
    if out.title.is_empty() {
        title.clone_into(&mut out.title);
    }
}
