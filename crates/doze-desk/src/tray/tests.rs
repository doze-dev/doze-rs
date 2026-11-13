//! Go's tray tests (`tray/tray_test.go`, `tray/needs_test.go`), each held as it was, and the menu's own.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::StackKey;
use crate::attention::{Attention, Fix, FixKind, Item};
use crate::conn::Conn;
use crate::view::StackView;
use doze_control::InstanceView;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Go's `now`: 2026-11-05 14:05 UTC.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_793_887_500)
}

fn stack(key: &str, name: &str, running: bool, insts: serde_json::Value) -> StackView {
    let mut st = StackView::new(StackKey::new(key));
    (st.name, st.dir, st.running) = (name.into(), format!("/code/{name}"), running);
    st.link.conn = Conn::Live;
    st.instances = Arc::new(serde_json::from_value::<Vec<InstanceView>>(insts).unwrap());
    st
}

fn stopped(key: &str, name: &str, days: u64) -> StackView {
    let mut st = stack(key, name, false, json!([]));
    st.last_up = Some(now() - Duration::from_secs(days * 24 * 3600));
    st
}

fn line(key: &str, name: &str, running: bool, sub: &str, trouble: bool) -> Line {
    Line {
        key: StackKey::new(key),
        name: name.into(),
        dir: format!("/code/{name}"),
        running,
        sub: sub.into(),
        trouble,
    }
}

fn four() -> Vec<StackView> {
    let mb = 1 << 20;
    vec![
        stack(
            "k-shop",
            "shop",
            true,
            json!([
                {"name": "api", "kind": "app", "state": "active", "ram": 48 * mb},
                {"name": "web", "kind": "app", "state": "active", "ram": 212 * mb, "https": "https://shop.acme.dev"},
                {"name": "db", "kind": "module", "state": "idle", "ram": 90 * mb},
                {"name": "migrate", "kind": "task", "state": "done"},
                {"name": "cache", "kind": "module", "state": "reaped"},
            ]),
        ),
        stack(
            "k-data",
            "data",
            true,
            json!([{"name": "search", "kind": "service", "state": "crashing", "restart_count": 2, "restart_max": 5}]),
        ),
        stopped("k-old", "old", 14),
        stopped("k-acme", "acme", 3),
    ]
}

#[test]
fn build() {
    let m = super::build(&four(), now());
    assert!(m.needs_you, "a crashing service turns the alert mark on");
    let want = vec![
        line(
            "k-data",
            "data",
            true,
            "search is crashing · try 2 of 5",
            true,
        ),
        line("k-shop", "shop", true, "2 awake · 1 dozing · 350 MB", false),
        line("k-acme", "acme", false, "up 3d ago", false),
        line("k-old", "old", false, "up 14d ago", false),
    ];
    assert_eq!(m.lines, want);
    assert_eq!(m.tooltip, "doze · 2 awake");
}

#[test]
fn build_with_nothing_running() {
    let m = super::build(&[], now());
    assert!(!m.needs_you && m.lines.is_empty() && m.tooltip == "doze");
    let mut st = stack("k", "shop", true, json!([]));
    st.link.conn = Conn::Down;
    let m = super::build(&[st], now());
    assert_eq!(m.lines.len(), 1);
    assert_eq!(m.lines[0].sub, "daemon not answering");
    let mut st = stack("k", "shop", true, json!([]));
    st.link.conn = Conn::Connecting;
    assert_eq!(super::build(&[st], now()).lines[0].sub, "connecting…");
    let never = stack("k", "", false, json!([]));
    let l = &super::build(&[never], now()).lines[0];
    assert_eq!((l.name.as_str(), l.sub.as_str()), ("a stack", "never up"));
}

#[test]
fn split() {
    let mut lines = Vec::new();
    for i in 0..10 {
        let mut l = line(&format!("u{i}"), &format!("up{i}"), true, "", i == 9);
        l.trouble = i == 9;
        lines.push(l);
    }
    for i in 0..7 {
        lines.push(line(
            &format!("d{i}"),
            &format!("down{i}"),
            false,
            "",
            false,
        ));
    }
    let (shown, more) = super::split(&lines);
    assert_eq!((shown.len(), more.len()), (8 + 1 + 5, 1 + 2));
    assert_eq!(
        (more[0].name.as_str(), more[1].name.as_str()),
        ("up8", "down5")
    );
    assert!(
        more.iter().all(|l| !l.trouble),
        "a stack in trouble went under More"
    );
    let (s, m) = super::split(&lines[..3]);
    assert_eq!((s.len(), m.len()), (3, 0), "a short list is all shown");
}

fn red(id: &str) -> Item {
    Item {
        id: id.into(),
        red: true,
        title: format!("{id} keeps crashing"),
        stack: "shop".into(),
        fixes: vec![
            Fix::new("Logs", FixKind::Show),
            Fix::new("Restart", FixKind::Do).verb("restart"),
        ],
        ..Item::default()
    }
}

fn yellow() -> Item {
    Item {
        id: "y".into(),
        ..Item::default()
    }
}

#[test]
fn needs_of() {
    let fixed = Item {
        id: "f".into(),
        red: true,
        fixed: true,
        ..Item::default()
    };
    let cases: Vec<(&str, Attention, &str, usize, &str)> = vec![
        ("nothing", Attention::default(), "", 0, ""),
        (
            "yellow only",
            Attention {
                yellow: 1,
                items: vec![yellow()],
                ..Attention::default()
            },
            "",
            0,
            "1 thing worth doing   ·   Open",
        ),
        (
            "red and yellow",
            Attention {
                red: 1,
                yellow: 2,
                items: vec![red("api"), yellow()],
                ..Attention::default()
            },
            "Needs you · 1",
            1,
            "and 2 things worth doing   ·   Open",
        ),
        (
            "four red, three shown",
            Attention {
                red: 4,
                items: vec![red("a"), red("b"), red("c"), red("d")],
                ..Attention::default()
            },
            "Needs you · 4",
            3,
            "",
        ),
        (
            "fixed is not listed",
            Attention {
                items: vec![fixed],
                ..Attention::default()
            },
            "",
            0,
            "",
        ),
    ];
    for (name, a, header, lines, worth) in cases {
        let n = super::needs_of(&a);
        assert_eq!(
            (n.header.as_str(), n.lines.len(), n.worth.as_str()),
            (header, lines, worth),
            "{name}"
        );
        if lines > 0 {
            let l = &n.lines[0];
            assert_eq!(
                l.fix.as_ref().map(|f| f.label.as_str()),
                Some("Restart"),
                "{name}"
            );
            assert_eq!(l.title, format!("{}   ·   shop", l.item.title), "{name}");
        }
    }
}

fn titles(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Item { title, .. } | Entry::Submenu { title, .. } => Some(title.clone()),
            Entry::Header(t) | Entry::Note(t) | Entry::Detail(t) => Some(t.clone()),
            Entry::Separator => None,
        })
        .collect()
}

fn sub<'a>(entries: &'a [Entry], starts: &str) -> &'a [Entry] {
    entries
        .iter()
        .find_map(|e| match e {
            Entry::Submenu { title, entries } if title.starts_with(starts) => {
                Some(entries.as_slice())
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn the_menu_has_needs_you_on_top_then_the_stacks_then_the_fixed_lines() {
    let stacks = four();
    let mut m = super::build(&stacks, now());
    let a = Attention {
        red: 1,
        yellow: 1,
        items: vec![red("api"), yellow()],
        ..Attention::default()
    };
    m.needs_you = a.red > 0;
    let e = menu(&m, &a, &stacks);
    let t = titles(&e);
    assert_eq!(t[0], "Needs you · 1");
    assert_eq!(t[1], "api keeps crashing   ·   shop");
    assert_eq!(t[2], "and 1 thing worth doing   ·   Open");
    assert_eq!(t[3], "Stacks");
    assert_eq!(t[4], "⚠︎  data   ·   search is crashing · try 2 of 5");
    assert!(t.ends_with(&[
        "Open doze".into(),
        "Search or do…".into(),
        "Settings…".into(),
        "Check for Updates…".into(),
        "Quit doze   ·   stacks keep running".into()
    ]));
    // The red item: its fix, then Show.
    let fix = titles(sub(&e, "api keeps crashing"));
    assert_eq!(fix, ["Restart", "Show in doze"]);
}

#[test]
fn a_running_stack_offers_its_address_its_services_and_its_actions() {
    let stacks = four();
    let m = super::build(&stacks, now());
    let e = menu(&m, &Attention::default(), &stacks);
    let shop = sub(&e, "●  shop");
    assert_eq!(
        titles(shop),
        [
            "Open in doze",
            "Open https://shop.acme.dev",
            "Services · 5",
            "All logs",
            "Sleep everything",
            "Restart",
            "Down"
        ]
    );
    let svcs = sub(shop, "Services");
    assert_eq!(
        titles(svcs),
        [
            "●  api   ·   awake",
            "●  web   ·   awake",
            "◐  db   ·   dozing",
            "▸  migrate   ·   not run",
            "·  cache   ·   asleep"
        ]
    );
    assert_eq!(
        titles(sub(svcs, "◐  db")),
        ["Sleep", "Open in doze", "Logs"]
    );
    assert_eq!(
        titles(sub(svcs, "·  cache")),
        ["Wake", "Open in doze", "Logs"]
    );
    assert_eq!(
        titles(sub(svcs, "▸  migrate")),
        ["Run", "Open in doze", "Logs"]
    );
    let data = sub(&e, "⚠︎  data");
    assert_eq!(
        titles(sub(sub(data, "Services"), "✗  search")),
        ["Restart", "Open in doze", "Logs"]
    );
    let acme = sub(&e, "○  acme");
    assert_eq!(
        titles(acme),
        ["Open in doze", "Up as last time", "Up everything"]
    );
}

#[test]
fn no_stacks_says_so_and_more_stacks_holds_the_rest() {
    let e = menu(&super::build(&[], now()), &Attention::default(), &[]);
    assert_eq!(titles(&e)[0], "No stacks yet · open a folder in doze");
    let many: Vec<StackView> = (0..9)
        .map(|i| stopped(&format!("k{i}"), &format!("s{i}"), i + 1))
        .collect();
    let e = menu(&super::build(&many, now()), &Attention::default(), &many);
    assert_eq!(titles(sub(&e, "More stacks · 4")).len(), 4);
}

#[test]
fn a_countdown_in_a_detail_does_not_change_the_signature() {
    let stacks = four();
    let m = super::build(&stacks, now());
    let mut a = Attention {
        red: 1,
        items: vec![red("api")],
        ..Attention::default()
    };
    a.items[0].detail = "boom · try 3 of 5, next in 4s".into();
    let first = menu(&m, &a, &stacks);
    a.items[0].detail = "boom · try 3 of 5, next in 3s".into();
    let second = menu(&m, &a, &stacks);
    assert_ne!(first, second);
    assert_eq!(signature(&first), signature(&second));
    a.items[0].title = "api did not become ready".into();
    assert_ne!(signature(&first), signature(&menu(&m, &a, &stacks)));
}

#[test]
fn the_text_indents_submenus_and_spells_the_keys() {
    let stacks = four();
    let t = text(&menu(
        &super::build(&stacks, now()),
        &Attention::default(),
        &stacks,
    ));
    assert!(t.contains("[Stacks]\n"));
    assert!(t.contains("\n    Open in doze\n"));
    assert!(t.contains("Open doze    ⌘⇧D\n"));
    assert!(t.contains("Settings…    ⌘,\n"));
}
