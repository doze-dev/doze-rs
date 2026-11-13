//! What each block of a console tab shows now: the resolver behind [`super::console`] (Go `view.resolver`).

use super::Ink;
use super::console::{Action, Cell, Col, Node, Row, Selection, Stat, block_id, wire_walk};
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
        }
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
        match w.kind {
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
            }
            NodeKind::Stream => self.stream(&mut out, n),
            NodeKind::Actions => {
                default_title(&mut out, "actions");
                out.actions = self.actions(n);
            }
            NodeKind::Note => {
                default_title(&mut out, "note");
                out.text.clone_from(&n.text);
            }
            NodeKind::Chart => self.chart(&mut out, n),
            NodeKind::Other(_) => {}
        }
        out
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
            .map(|row| Row {
                key: row.cell(first).to_owned(),
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
        if !rows.is_empty() {
            out.selected = self
                .sel
                .get(&out.id)
                .copied()
                .unwrap_or(0)
                .min(rows.len() - 1);
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
        out.stats = p
            .columns
            .iter()
            .map(|col| {
                let (t, tone) = words::cell(col, row);
                let (value, ink) = if t.is_empty() {
                    ("—".to_owned(), Ink::Dim)
                } else {
                    (t.into_owned(), tone.into())
                };
                Stat {
                    label: col.title.clone(),
                    value,
                    ink,
                }
            })
            .collect();
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
                if let Some(t) = tables
                    .first()
                    .filter(|_| res.is_empty() && !a.kind.is_empty())
                {
                    res = self.selected_key(&block_id(t), &t.panel);
                }
                Action {
                    id: a.id.clone(),
                    label: a.label.clone(),
                    key: words::action_key(&a.id).unwrap_or('?'),
                    destructive: a.destructive,
                    hint: a.input_hint.clone(),
                    resource: res,
                }
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
    }
}

fn default_title(out: &mut Node, title: &str) {
    if out.title.is_empty() {
        title.clone_into(&mut out.title);
    }
}
