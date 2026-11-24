//! Every real module's console, resolved as the app draws it: the `consoles` fixture (apps/doze-app/fixtures/
//! consoles.ts) holds the spec each module publishes (kvrocks, valkey, temporal, postgres, the two echo modules, Go's
//! kafka and aws) and a module newer than this doze. Each tab and each Overview extension must resolve with nothing
//! left blank: a block of a kind this doze does not know says so instead of leaving a hole.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use doze_control::fixture::{Script, Step};
use doze_desk::ConsoleView;
use doze_desk::convert;
use doze_desk::page::console::{
    ConsolePage, Node, Selection, Views, console_page, console_page_viewed, extensions,
};
use doze_display::NodeKind;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

mod picks;

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
    // A console still reading, or one that failed, has no spec: nothing to resolve yet.
    for (name, c) in all.iter().filter(|(_, c)| c.spec.is_some()) {
        for tab in tabs(c) {
            for p in [
                console_page(name, &tab, Some(c), &Selection::new()),
                live(name, &tab, c, &Selection::new()),
            ] {
                assert!(!p.missing && !p.reading, "{name} {tab}: not drawn");
                assert!(
                    blank(&p.body).is_empty(),
                    "{name} {tab}: blank blocks {:?}",
                    blank(&p.body)
                );
            }
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

#[test]
fn an_action_on_the_server_is_not_put_on_a_tables_row() {
    let all = consoles();
    let p = console_page("cache", "console:_", Some(&all["cache"]), &Selection::new());
    let acts: Vec<_> = p
        .body
        .walk()
        .into_iter()
        .flat_map(|n| n.actions.clone())
        .collect();
    assert!(!acts.is_empty());
    for a in acts {
        assert_eq!(a.resource, "", "{} is on the server, not on db0", a.label);
    }
    // A namespace action still names the selected namespace, and is at that row, not in the actions block.
    let p = live("store", "console:_", &all["store"], &Selection::new());
    let flush = p
        .body
        .walk()
        .into_iter()
        .flat_map(|n| n.row_actions.clone())
        .find(|a| a.id == "flush")
        .unwrap();
    assert_eq!(flush.resource, "(default)");
    assert!(
        !p.body
            .walk()
            .iter()
            .any(|n| n.actions.iter().any(|a| a.id == "flush")),
        "the namespace's action is at its row only"
    );
}

/// The tab as the app draws it: tables ordered and filtered (none here), actions at their rows.
fn live(name: &str, tab: &str, c: &ConsoleView, sel: &Selection) -> ConsolePage {
    console_page_viewed(name, tab, Some(c), sel, &Views::new())
}

fn block(p: &ConsolePage, kind: NodeKind) -> &Node {
    p.body.walk().into_iter().find(|n| n.kind == kind).unwrap()
}

#[test]
fn a_session_action_is_at_the_row_and_in_the_detail_and_the_actions_card_goes() {
    let all = consoles();
    let mut sel = Selection::new();
    sel.insert("table:sessions".to_owned(), 2);
    let p = live("db", "console:activity", &all["db"], &sel);
    let sessions = block(&p, NodeKind::Table);
    let ids: Vec<&str> = sessions.row_actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["cancel", "terminate"]);
    assert!(sessions.row_actions.iter().all(|a| a.resource == "4410"));
    assert!(sessions.detailed, "a detail shows the selected session");
    let detail = block(&p, NodeKind::Detail);
    assert_eq!(detail.row_actions.len(), 2, "the detail offers them too");
    let acts = block(&p, NodeKind::Actions);
    assert!(
        acts.moved && acts.actions.is_empty(),
        "nothing left for the actions card"
    );
    // The keys still name both letters.
    assert!(p.keys.iter().any(|(k, _)| k == "C"));
    assert!(p.keys.iter().any(|(k, _)| k == "T"));
    // On the overview nothing is picked: the actions stay where the module put them.
    for (_, n) in extensions(&all["store"]) {
        assert!(n.walk().iter().all(|b| b.row_actions.is_empty()));
    }
}

#[test]
fn columns_are_sized_for_what_they_hold_and_only_long_words_grow() {
    let all = consoles();
    let p = live(
        "gauge",
        "console:overview",
        &all["gauge"],
        &Selection::new(),
    );
    let keys = block(&p, NodeKind::Table);
    let grows: Vec<&str> = keys
        .cols
        .iter()
        .filter(|c| c.grow)
        .map(|c| c.key.as_str())
        .collect();
    assert_eq!(grows, ["key"], "the long keys take the room");
    let ty = keys.cols.iter().find(|c| c.key == "type").unwrap();
    assert_eq!(
        ty.chars,
        "string".len(),
        "a column is as wide as its widest cell"
    );
    let size = keys.cols.iter().find(|c| c.key == "size").unwrap();
    assert_eq!(
        size.chars,
        "SIZE".len() + 2,
        "or its title, with room for the sort mark"
    );
    // A table of short columns keeps them together: none grows.
    let p = live("events", "console:_", &all["events"], &Selection::new());
    let topics = p
        .body
        .walk()
        .into_iter()
        .find(|n| n.id == "table:topics")
        .unwrap();
    assert!(topics.cols.iter().all(|c| !c.grow), "{:?}", topics.cols);
}

#[test]
fn a_size_reads_in_its_own_unit() {
    let all = consoles();
    let p = live(
        "gauge",
        "console:overview",
        &all["gauge"],
        &Selection::new(),
    );
    let keys = block(&p, NodeKind::Table);
    let at = keys.cols.iter().position(|c| c.key == "size").unwrap();
    assert_eq!(keys.rows[0].cells[at].text, "120 B", "not 0 MB");
    assert_eq!(keys.rows[1].cells[at].text, "1.1 KB");
}

#[test]
fn a_limit_folds_into_the_figure_before_it() {
    let all = consoles();
    let p = live("db", "console:activity", &all["db"], &Selection::new());
    let stats = &block(&p, NodeKind::Stat).stats;
    let labels: Vec<&str> = stats.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Active", "Idle", "Waiting", "Connections", "Longest"]
    );
    let c = &stats[3];
    assert_eq!((c.value.as_str(), c.of.as_str()), ("16", "100"));
    assert_eq!(c.fill, Some(0.16));
    // Words as the limit: no meter. A figure not read yet says why.
    let p = live("cache", "console:_", &all["cache"], &Selection::new());
    let stats = &block(&p, NodeKind::Stat).stats;
    let mem = stats.iter().find(|s| s.label == "Memory").unwrap();
    assert_eq!(mem.of, "256 MB");
    assert!(
        mem.fill.is_some_and(|f| f > 0.04 && f < 0.05),
        "{:?}",
        mem.fill
    );
    let hits = stats.iter().find(|s| s.label == "Hits").unwrap();
    assert_eq!(hits.value, "—");
    assert!(!hits.note.is_empty());
}

#[test]
fn a_table_is_ordered_and_filtered_as_its_view_says() {
    use doze_desk::page::console::{TableView, Views, console_page_viewed};
    let all = consoles();
    let mut views = Views::new();
    let mut v = TableView::default();
    v.click(2);
    v.click(2);
    assert_eq!(v.sort, Some((2, true)), "a second click orders downwards");
    v.filter = "PROFILE:WITH".to_owned();
    views.insert("keys".to_owned(), v.clone());
    let sel = Selection::new();
    let p = console_page_viewed(
        "gauge",
        "console:overview",
        Some(&all["gauge"]),
        &sel,
        &views,
    );
    let keys = block(&p, NodeKind::Table);
    assert_eq!(keys.total, 1000);
    assert_eq!(keys.rows.len(), 143, "one key in seven has the long name");
    assert!(
        keys.rows
            .iter()
            .all(|r| r.cells[0].text.contains("profile:with"))
    );
    assert_eq!(keys.cols[2].sort, Some(true));
    // Ordered by the bytes, not the words: 87 KB before 9.5 KB.
    let raw: Vec<i64> = keys
        .rows
        .iter()
        .map(|r| ((r.at * 977) % 90000 + 120) as i64)
        .collect();
    assert!(raw.windows(2).all(|w| w[0] >= w[1]), "downwards by size");
    // The selection is the module's row, wherever it is shown.
    assert!(keys.rows.iter().any(|r| r.at == keys.selected));
    let mut third = v;
    third.click(2);
    assert_eq!(
        third.sort, None,
        "a third click is the module's order again"
    );
}

#[test]
fn an_empty_stream_is_a_line_and_a_stream_with_lines_takes_the_room() {
    let all = consoles();
    let p = live("gauge", "console:errors", &all["gauge"], &Selection::new());
    let s = block(&p, NodeKind::Stream);
    assert!(s.rows.is_empty() && !s.flexible);
    let p = live(
        "gauge",
        "console:requests",
        &all["gauge"],
        &Selection::new(),
    );
    let s = block(&p, NodeKind::Stream);
    assert!(!s.rows.is_empty() && s.flexible);
    assert!(
        !p.keys.iter().any(|(_, w)| w == "select"),
        "no table here: nothing to select"
    );
}

#[test]
fn a_chart_says_its_scale_and_what_it_runs_over() {
    let all = consoles();
    let p = live(
        "gauge",
        "console:overview",
        &all["gauge"],
        &Selection::new(),
    );
    let c = block(&p, NodeKind::Chart);
    assert_eq!(c.span, "last 120 readings");
    assert!(!c.low.is_empty() && !c.high.is_empty());
    let (lo, hi): (f64, f64) = (c.low.parse().unwrap(), c.high.parse().unwrap());
    assert!(c.values.iter().all(|v| *v >= lo && *v <= hi));
}
