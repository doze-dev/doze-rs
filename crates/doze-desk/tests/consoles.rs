//! Every real module's console, resolved as the app draws it: the `consoles` fixture (apps/doze-app/fixtures/
//! consoles.ts) holds the spec each module publishes (kvrocks, valkey, temporal, postgres, the two echo modules, Go's
//! kafka and aws) and a module newer than this doze. Each tab and each Overview extension must resolve with nothing
//! left blank: a block of a kind this doze does not know says so instead of leaving a hole.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use doze_control::fixture::{Script, Step};
use doze_desk::ConsoleView;
use doze_desk::convert;
use doze_desk::page::console::{Node, Selection, console_page, extensions};
use doze_display::NodeKind;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Each instance's console as the hub would hold it once the script played.
fn consoles() -> BTreeMap<String, ConsoleView> {
    let s = Script::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/doze-app/fixtures/scripts/consoles.ndjson"),
    )
    .unwrap();
    let mut out: BTreeMap<String, ConsoleView> = BTreeMap::new();
    for step in &s.steps {
        let Step::Frame(f) = step else { continue };
        let Some(c) = &f.console else { continue };
        let v = out.entry(c.instance.clone()).or_default();
        if let Some(spec) = &c.spec {
            v.words = Some(Arc::new(convert::console_spec(spec)));
            v.spec = Some(Arc::new(spec.clone()));
            v.actions.clone_from(&c.actions);
            v.state.clone_from(&c.state);
        }
        if !c.rows.is_empty() {
            v.tables.insert(
                c.panel.clone(),
                c.rows.iter().map(convert::console_row).collect(),
            );
        }
        if let Some(r) = &c.row {
            v.streams
                .entry(c.panel.clone())
                .or_default()
                .push(convert::console_row(r));
        }
    }
    out
}

fn tabs(c: &ConsoleView) -> Vec<String> {
    let spec = c.spec.as_ref().unwrap();
    if spec.tabs.is_empty() {
        vec!["console:_".to_owned()]
    } else {
        spec.tabs
            .iter()
            .map(|t| format!("console:{}", t.id))
            .collect()
    }
}

/// Blocks that would draw nothing: no rows, no stats, no words and no reason why.
fn blank(n: &Node) -> Vec<String> {
    let mut out = Vec::new();
    for b in n.walk() {
        let empty = match b.kind {
            NodeKind::Row | NodeKind::Col | NodeKind::Actions | NodeKind::Detail => false,
            NodeKind::Table | NodeKind::Stream => b.cols.is_empty(),
            NodeKind::Stat => b.stats.is_empty(),
            NodeKind::Note => b.text.is_empty(),
            NodeKind::Chart => b.values.is_empty(),
            NodeKind::Other(_) => true,
        };
        if empty && b.missing.is_empty() {
            out.push(format!("{} {}", b.kind.as_str(), b.id));
        }
    }
    out
}

#[test]
fn every_modules_console_resolves() {
    let all = consoles();
    for want in [
        "store", "cache", "flows", "db", "echo", "echo-go", "events", "files", "gauge",
    ] {
        assert!(
            all.contains_key(want),
            "{want} has no console in the fixture"
        );
    }
    for (name, c) in &all {
        for tab in tabs(c) {
            let p = console_page(name, &tab, Some(c), &Selection::new());
            assert!(!p.missing && !p.reading, "{name} {tab}: not drawn");
            assert!(
                blank(&p.body).is_empty(),
                "{name} {tab}: blank blocks {:?}",
                blank(&p.body)
            );
        }
        for (after, n) in extensions(c) {
            assert!(
                blank(&n).is_empty(),
                "{name}'s Overview block after {after}: blank {:?}",
                blank(&n)
            );
        }
    }
}

#[test]
fn a_block_this_doze_does_not_know_says_it_needs_a_newer_doze() {
    let all = consoles();
    let g = &all["gauge"];
    let p = console_page("gauge", "console:overview", Some(g), &Selection::new());
    let unknown: Vec<&Node> = p
        .body
        .walk()
        .into_iter()
        .filter(|n| matches!(n.kind, NodeKind::Other(_)))
        .collect();
    assert_eq!(unknown.len(), 1);
    assert!(
        unknown[0].missing.contains("needs a newer doze"),
        "{:?}",
        unknown[0].missing
    );
    let ext = extensions(g);
    assert!(ext.iter().any(|(_, n)| {
        n.walk()
            .iter()
            .any(|b| b.missing.contains("needs a newer doze"))
    }));
}

#[test]
fn a_thousand_row_table_and_a_full_stream_resolve_whole() {
    let all = consoles();
    let p = console_page(
        "gauge",
        "console:overview",
        Some(&all["gauge"]),
        &Selection::new(),
    );
    let keys = p
        .body
        .walk()
        .into_iter()
        .find(|n| n.kind == NodeKind::Table)
        .unwrap();
    assert_eq!(
        keys.rows.len(),
        1000,
        "every row is there for the virtual list to show"
    );
    let p = console_page(
        "gauge",
        "console:requests",
        Some(&all["gauge"]),
        &Selection::new(),
    );
    let log = p
        .body
        .walk()
        .into_iter()
        .find(|n| n.kind == NodeKind::Stream)
        .unwrap();
    // The newest row is last, where the app keeps the view.
    assert_eq!(
        log.rows.last().unwrap().cells.first().unwrap().text,
        "14:04:39"
    );
}
