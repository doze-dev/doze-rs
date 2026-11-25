//! The tree as a person opens it: lazily, a page at a time, filtered through the module, refusals said in place.
#![allow(clippy::unwrap_used)]

use super::*;
use std::collections::BTreeMap;

fn node(id: &str, leaf: bool, info: &[(&str, &str)]) -> TreeNodeView {
    TreeNodeView {
        id: id.into(),
        label: id.rsplit('/').next().unwrap().into(),
        leaf,
        info: info
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn page(nodes: Vec<TreeNodeView>, next: &str) -> ChildrenView {
    ChildrenView {
        nodes,
        next: next.into(),
    }
}

fn labels(t: &TreeState) -> Vec<String> {
    t.rows()
        .into_iter()
        .map(|r| match r {
            TreeRow::Node {
                label,
                depth,
                open,
                leaf,
                info,
                ..
            } => {
                let mark = if leaf {
                    " "
                } else if open {
                    "▾"
                } else {
                    "▸"
                };
                format!("{}{mark}{label} {info}", "  ".repeat(depth))
                    .trim_end()
                    .to_owned()
            }
            TreeRow::More { depth, .. } => format!("{}more", "  ".repeat(depth)),
            TreeRow::Loading { depth } => format!("{}…", "  ".repeat(depth)),
            TreeRow::Note { depth, text, .. } => format!("{}{text}", "  ".repeat(depth)),
        })
        .collect()
}

#[test]
fn folders_open_lazily_and_page() {
    let mut t = TreeState::default();
    let top = t.start().unwrap();
    assert_eq!(t.start(), None, "asked once");
    assert_eq!(labels(&t), ["…"]);
    t.got(
        &top,
        page(
            vec![node("app", false, &[]), node("app_test", false, &[])],
            "",
        ),
    );
    let f = t.toggle("app").unwrap();
    assert_eq!(labels(&t), ["▾app", "  …", "▸app_test"]);
    t.got(
        &f,
        page(vec![node("app/orders", true, &[("rows", "12431")])], "c1"),
    );
    assert_eq!(
        labels(&t),
        ["▾app", "   orders 12,431 rows", "  more", "▸app_test"]
    );
    let m = t.more("app").unwrap();
    assert_eq!(m.cursor, "c1");
    t.got(
        &m,
        page(vec![node("app/customers", true, &[("rows", "1")])], ""),
    );
    assert_eq!(labels(&t)[2], "   customers 1 row");
    assert_eq!(t.toggle("app"), None, "closing asks nothing");
    assert_eq!(
        t.toggle("app"),
        None,
        "reopening a folder read asks nothing"
    );
}

#[test]
fn the_filter_searches_through_the_module_and_drops_stale_pages() {
    let mut t = TreeState::default();
    let a = t.set_filter("ord").unwrap();
    let b = t.set_filter("orders").unwrap();
    t.got(&a, page(vec![node("x", true, &[])], ""));
    assert_eq!(labels(&t), ["…"], "a page for an old filter is dropped");
    t.got(&b, page(vec![], ""));
    assert_eq!(labels(&t), ["Nothing matches “orders”."]);
    assert_eq!(t.set_filter(""), None);
    assert!(t.rows().is_empty(), "the top was never read");
}

#[test]
fn a_refusal_is_said_where_the_folder_would_be() {
    let mut t = TreeState::default();
    let top = t.start().unwrap();
    t.refused(
        &top,
        Refusal {
            message: "db is asleep. Opening a value wakes it.".into(),
            code: "asleep".into(),
        },
    );
    assert_eq!(
        labels(&t),
        ["Asleep: listed again as it wakes. Listing never wakes it."]
    );
    assert!(t.start().is_some(), "asked again later");
}

#[test]
fn a_leaf_is_named_by_its_path() {
    let mut t = TreeState::default();
    let top = t.start().unwrap();
    t.got(
        &top,
        page(vec![node("app", false, &[]), node("b", false, &[])], ""),
    );
    let f = t.toggle("app").unwrap();
    t.got(
        &f,
        page(
            vec![node("app/x", true, &[]), node("app/orders", true, &[])],
            "",
        ),
    );
    assert_eq!(t.path("app/orders"), ["app", "orders"]);
    assert!(t.path("nope").is_empty());
}

#[test]
fn info_in_words() {
    assert_eq!(thousands(1_234_567), "1,234,567");
    let n = node("k", true, &[("bytes", "2048"), ("type", "hash")]);
    assert_eq!(info(&n), "2.0 KB · hash");
}
