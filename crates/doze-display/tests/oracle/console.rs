//! The console cases: layouts, blocks, details, cells, tabs and rates over a generated module console.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use doze_display as d;
use doze_display::{
    Column, ConsoleRow, ConsoleSpec, ConsoleTab, Instance, Kind, Node, NodeKind, Panel, PanelKind,
    Role,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, Default)]
#[serde(default)]
struct SpecIn {
    panels: Vec<PanelIn>,
    tabs: Vec<TabIn>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PanelIn {
    id: String,
    title: String,
    kind: String,
    columns: Vec<ColumnIn>,
    select: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ColumnIn {
    key: String,
    title: String,
    role: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TabIn {
    id: String,
    title: String,
    body: NodeIn,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NodeIn {
    kind: String,
    id: String,
    title: String,
    weight: u32,
    panel: String,
    from: String,
    children: Vec<NodeIn>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RowIn {
    cells: HashMap<String, String>,
    tone: String,
}

#[derive(Deserialize)]
struct Given {
    spec: Option<SpecIn>,
    #[serde(default)]
    tables: HashMap<String, Vec<RowIn>>,
    #[serde(default)]
    sel: HashMap<String, usize>,
}

#[derive(Deserialize)]
struct RateSet {
    panel: String,
    row: usize,
    key: String,
    value: String,
}

fn node(n: NodeIn) -> Node {
    Node {
        kind: NodeKind::parse(&n.kind),
        id: n.id,
        title: n.title,
        weight: n.weight,
        panel: n.panel,
        from: n.from,
        children: n.children.into_iter().map(node).collect(),
    }
}

fn spec(s: SpecIn) -> ConsoleSpec {
    let panel = |p: PanelIn| Panel {
        kind: PanelKind::parse(&p.kind),
        columns: p
            .columns
            .into_iter()
            .map(|c| Column {
                role: Role::parse(&c.role),
                key: c.key,
                title: c.title,
            })
            .collect(),
        id: p.id,
        title: p.title,
        select: p.select,
    };
    ConsoleSpec {
        panels: s.panels.into_iter().map(panel).collect(),
        tabs: s
            .tabs
            .into_iter()
            .map(|t| ConsoleTab {
                id: t.id,
                title: t.title,
                body: node(t.body),
            })
            .collect(),
    }
}

fn tree(n: &Node) -> Value {
    let children: Vec<Value> = n.children.iter().map(tree).collect();
    json!({"kind": n.kind.as_str(), "id": n.id, "title": n.title, "weight": n.weight, "panel": n.panel, "from": n.from, "children": children})
}

pub(crate) fn say(input: &Value) -> Value {
    let given = Given::deserialize(&input["given"]).expect("a console");
    let steps: Vec<Vec<RateSet>> = Vec::deserialize(&input["steps"]).expect("rate steps");
    let spec = given.spec.map(spec);
    let spec = spec.as_ref();
    let tables: HashMap<String, Vec<ConsoleRow>> = given
        .tables
        .into_iter()
        .map(|(k, rows)| {
            (
                k,
                rows.into_iter()
                    .map(|r| ConsoleRow {
                        cells: r.cells,
                        tone: r.tone,
                    })
                    .collect(),
            )
        })
        .collect();
    let mut bodies = serde_json::Map::new();
    let mut walked = Vec::new();
    for tab in ["console:_", "console:main", "console:more", "logs"] {
        let Some(body) = d::console_body(spec, tab) else {
            bodies.insert(tab.to_owned(), Value::Null);
            continue;
        };
        bodies.insert(tab.to_owned(), tree(&body));
        for n in body.walk() {
            let detail: Vec<Value> = d::detail_rows(spec, tab, &tables, &given.sel, n)
                .iter()
                .map(|(k, v)| json!([k, v]))
                .collect();
            walked.push(json!([
                n.block_id(),
                n.share(),
                n.flexible(),
                n.has_stream(),
                detail
            ]));
        }
    }
    let tabs: Vec<Value> = d::console_tabs(spec)
        .iter()
        .map(|t| json!([t.id, t.title]))
        .collect();
    let mut per_instance = Vec::new();
    for front in ["", "console", "web"] {
        for kind in [Kind::Module, Kind::Task] {
            let v = Instance {
                kind,
                has_console: true,
                console_front: front.to_owned(),
                ..Instance::default()
            };
            let ts: Vec<Value> = d::tabs(&v, spec)
                .iter()
                .map(|t| json!([t.id, t.title]))
                .collect();
            per_instance.push(if ts.is_empty() {
                Value::Null
            } else {
                json!(ts)
            });
        }
    }
    json!({
        "bodies": bodies, "walked": walked, "console_tabs": tabs, "tabs": per_instance,
        "cells": cells(spec, &tables), "rates": rates(spec, tables, &steps),
    })
}

fn cells(spec: Option<&ConsoleSpec>, tables: &HashMap<String, Vec<ConsoleRow>>) -> Vec<Value> {
    let mut out = Vec::new();
    for p in spec.map_or(&[][..], |s| &s.panels) {
        for row in tables.get(&p.id).map_or(&[][..], Vec::as_slice) {
            for col in &p.columns {
                let (text, tone) = d::cell(col, row);
                out.push(json!([text, tone.as_str()]));
            }
        }
    }
    out
}

const STEPS: [Duration; 5] = [
    Duration::ZERO,
    Duration::from_secs(1),
    Duration::from_millis(2500),
    Duration::from_millis(2500),
    Duration::from_secs(4),
];

fn rates(
    spec: Option<&ConsoleSpec>,
    mut tables: HashMap<String, Vec<ConsoleRow>>,
    steps: &[Vec<RateSet>],
) -> Vec<Value> {
    let mut said = Vec::new();
    let Some(s) = spec else {
        return said;
    };
    let t0 = Instant::now();
    let mut prev = HashMap::new();
    let mut steps = steps.iter();
    for p in &s.panels {
        for step in STEPS {
            for set in steps.next().expect("a rate step") {
                let row = tables
                    .get_mut(&set.panel)
                    .and_then(|rows| rows.get_mut(set.row))
                    .expect("the row a step writes");
                row.cells.insert(set.key.clone(), set.value.clone());
            }
            // Go's Rates hands back new rows and leaves the frame it read; here the frame is written in place, so
            // each step reads a fresh copy of the frame, as a surface reads each frame the module sends.
            let mut rows = tables.get(&p.id).cloned().unwrap_or_default();
            d::rates(spec, &p.id, &mut rows, &mut prev, t0 + step);
            let mut out = Vec::new();
            for row in &rows {
                let mut cells: Vec<_> = row.cells.iter().collect();
                cells.sort();
                out.extend(cells.into_iter().map(|(k, v)| json!([k, v])));
                out.push(json!(["|", row.tone]));
            }
            said.push(json!(out));
        }
    }
    said
}
