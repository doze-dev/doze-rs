#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::view::StackView;
use doze_control::InstanceView;
use doze_projects::StackKey;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_791_727_480)
}

fn stack(key: &str, running: bool, insts: serde_json::Value) -> StackView {
    let mut st = StackView::new(StackKey::new(key));
    st.name = key.to_owned();
    st.dir = format!("/w/{key}");
    st.running = running;
    st.instances = Arc::new(serde_json::from_value::<Vec<InstanceView>>(insts).unwrap());
    st
}

fn shop() -> StackView {
    stack(
        "shop",
        true,
        json!([
            {"name": "api", "kind": "app", "state": "active", "url": "http://api.shop.doze", "holders": [{"kind": "connection", "count": 2}]},
            {"name": "cache", "kind": "module", "engine": "redis", "state": "idle", "url": "redis://cache.shop.doze:6379"},
            {"name": "db", "kind": "module", "engine": "postgres", "state": "active", "url": "postgres://shop@db.shop.doze:5432/shop", "env_var": "DATABASE_URL"},
            {"name": "worker", "kind": "app", "state": "crashing", "last_error": "boom"},
            {"name": "migrate", "kind": "task"}
        ]),
    )
}

fn ask<'a>(q: &'a str, scope: Option<&'a Scope>) -> Ask<'a> {
    Ask {
        current: "shop",
        inst: "",
        scope,
        recent: &[],
        cmds: &[],
        query: q,
        now: now(),
        extra: &[],
    }
}

fn titles(p: &Palette) -> Vec<&str> {
    p.sections.iter().map(|s| s.title.as_str()).collect()
}

#[test]
fn nothing_typed_is_what_needs_you_then_suggested_then_stacks() {
    let stacks = [shop(), stack("acme", false, json!([]))];
    let p = palette(&stacks, &ask("", None));
    assert_eq!(titles(&p), ["Needs you", "Suggested", "Stacks"]);
    assert_eq!(p.sections[0].rows[0].label, "worker");
    let sts = &p.sections[2].rows;
    assert_eq!(
        (sts[0].label.as_str(), sts[1].note.as_str()),
        ("shop", "stopped")
    );
    assert_eq!(sts[2].doing.kind, DoKind::Folder);
}

#[test]
fn a_verb_finds_what_it_applies_to_then_did_you_mean() {
    let stacks = [shop()];
    let p = palette(&stacks, &ask("stop ca", None));
    assert_eq!(titles(&p), ["Sleep", "Did you mean"]);
    let r = &p.sections[0].rows[0];
    assert_eq!(
        (r.label.as_str(), r.cli.as_str(), r.key.as_str()),
        ("cache", "doze sleep cache", "⏎ sleep now")
    );
    // Something holds api: sleeping it asks first.
    let p = palette(&stacks, &ask("sleep api", None));
    assert_eq!(p.sections[0].rows[0].key, "⏎ asks first");
    assert!(p.sections[0].rows[0].doing.ask);
}

#[test]
fn a_stack_verb_marks_the_name_where_it_sits() {
    let stacks = [shop()];
    let p = palette(&stacks, &ask("restart sh", None));
    let r = &p.sections[0].rows[0];
    assert_eq!(r.label, "Restart shop");
    assert_eq!(r.at, [8, 9]);
}

#[test]
fn nothing_found_offers_the_closest_and_never_runs_the_guess() {
    let stacks = [shop(), stack("acme", false, json!([]))];
    let p = palette(&stacks, &ask("restart cachee", None));
    let n = p.nothing.unwrap();
    assert_eq!(
        (n.closest.as_str(), n.r#where.as_str(), n.fill.as_str()),
        ("cache", "shop or acme", "restart cache")
    );
    assert!(n.doing.is_none());
    // By engine: "redis" is cache's.
    let n = palette(&stacks, &ask("rediss", None)).nothing.unwrap();
    assert_eq!(
        (n.closest.as_str(), n.why.as_str()),
        ("cache", "redis, in shop")
    );
    assert!(n.doing.is_some());
}

#[test]
fn a_name_then_words_is_a_command_or_a_log_search() {
    let stacks = [shop()];
    let p = palette(&stacks, &ask("db restart", None));
    assert_eq!(p.sections[0].rows[0].label, "Restart db");
    let p = palette(&stacks, &ask("api timeout", None));
    let r = &p.sections[0].rows[0];
    assert_eq!(
        (
            r.doing.page.as_str(),
            r.doing.text.as_str(),
            r.doing.target.as_str()
        ),
        ("logs", "timeout", "api")
    );
}

#[test]
fn scopes_narrow_and_say_where() {
    let stacks = [shop()];
    let db = Scope {
        key: "shop".into(),
        name: Some("db".into()),
    };
    let p = palette(&stacks, &ask("", Some(&db)));
    assert_eq!(p.crumbs, ["shop", "db"]);
    assert_eq!(p.sections[0].rows[0].label, "Sleep db");
    let p = palette(&stacks, &ask("psql", Some(&db)));
    assert!(p.sections[0].rows[0].note.starts_with("psql \"postgres://"));
    let p = palette(&stacks, &ask("zzz", Some(&db)));
    assert_eq!(p.nothing.unwrap().r#where, "shop › db");
    assert_eq!(
        db.out(),
        Some(Scope {
            key: "shop".into(),
            name: None
        })
    );
    assert_eq!(serde_json::to_value(&db).unwrap(), json!("shop#db"));
}

#[test]
fn recents_are_newest_first_once_and_capped() {
    let mut list = Vec::new();
    for n in ["a", "b", "c", "a"] {
        list = opened(&list, "k", n);
    }
    let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["a", "c", "b"]);
    let mut cmds = Vec::new();
    for i in 0..9 {
        let c = RecentCommand {
            glyph: "↻".into(),
            label: format!("c{}", i % 7),
            note: String::new(),
            doing: Doing::default(),
        };
        cmds = remember(&cmds, c);
    }
    assert_eq!(cmds.len(), 6);
    assert_eq!(cmds[0].label, "c1");
    // A kept command reads back in the palette with its glyph.
    let stacks = [shop()];
    let a = Ask {
        cmds: &cmds,
        ..ask("", None)
    };
    let p = palette(&stacks, &a);
    let recent = p.sections.iter().find(|s| s.title == "Recent").unwrap();
    assert_eq!((recent.rows.len(), recent.rows[0].glyph), (4, "↻"));
}

#[test]
fn edit_distance_is_levenshtein() {
    assert_eq!(edit_distance("kitten", "sitting"), 3);
    assert_eq!(edit_distance("", "abc"), 3);
    assert_eq!(edit_distance("cachee", "cache"), 1);
    assert_eq!(edit_distance("ünï", "uni"), 2);
}

#[test]
fn clients_quote_as_go_does() {
    assert_eq!(
        client_line("postgres://a@b/c").as_deref(),
        Some("psql \"postgres://a@b/c\"")
    );
    assert_eq!(
        client_line("redis://x").as_deref(),
        Some("redis-cli -u redis://x")
    );
    assert_eq!(client_line("http://x"), None);
}
