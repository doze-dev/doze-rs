//! The console's working blocks (module protocol minor 2) before this doze draws them: each new kind resolves to the
//! "needs a newer doze" placeholder where it sits, and the rest of its tab resolves as before. Steps 2 to 4 replace
//! each placeholder with the block itself, one kind at a time.

use doze_control::{ConsoleNode, ConsolePanel, ConsoleSpec, ConsoleTab};
use doze_desk::ConsoleView;
use doze_desk::convert;
use doze_desk::page::console::{Selection, console_page};
use doze_display::NodeKind;
use std::sync::Arc;

fn view() -> ConsoleView {
    let panel = |id: &str, kind: &str| ConsolePanel {
        id: id.into(),
        title: id.into(),
        kind: kind.into(),
        source: "check".into(),
        ..ConsolePanel::default()
    };
    let node = |kind: &str, panel: &str| ConsoleNode {
        kind: kind.into(),
        id: kind.into(),
        panel: panel.into(),
        ..ConsoleNode::default()
    };
    let spec = ConsoleSpec {
        panels: vec![panel("health", "checks"), panel("t", "table")],
        tabs: vec![ConsoleTab {
            id: "work".into(),
            title: "Work".into(),
            body: ConsoleNode {
                kind: "col".into(),
                children: vec![
                    ConsoleNode {
                        from: "table".into(),
                        ..node("value", "t")
                    },
                    ConsoleNode {
                        keys: vec!["lag".into()],
                        by: "name".into(),
                        ..node("series", "t")
                    },
                    node("checks", "health"),
                    node("tree", "t"),
                    node("note", ""),
                ],
                ..ConsoleNode::default()
            },
        }],
        ..ConsoleSpec::default()
    };
    ConsoleView {
        words: Some(Arc::new(convert::console_spec(&spec))),
        spec: Some(Arc::new(spec)),
        ..ConsoleView::default()
    }
}

#[test]
fn each_block_not_drawn_yet_is_a_placeholder() {
    let v = view();
    let p = console_page("db", "console:work", Some(&v), &Selection::new());
    assert!(!p.missing, "the tab resolves");
    let nodes = p.body.walk();
    let unknown: Vec<String> = nodes
        .iter()
        .filter_map(|n| match &n.kind {
            NodeKind::Other(k) => {
                assert!(
                    n.missing.contains("needs a newer doze"),
                    "{k}: {:?}",
                    n.missing
                );
                Some(k.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        unknown,
        ["series", "checks"],
        "the value viewer and the tree are drawn (step 2); the rest wait"
    );
    let value = nodes.iter().find(|n| n.kind == NodeKind::Value).unwrap();
    assert!(
        value.missing.contains("not on this tab"),
        "a value following no block on the tab says so: {:?}",
        value.missing
    );
    let tree = nodes.iter().find(|n| n.kind == NodeKind::Tree).unwrap();
    assert_eq!((tree.panel.as_str(), tree.paired), ("t", false));
    assert!(
        nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Note) && n.missing.is_empty()),
        "a known block beside them is drawn as before"
    );
}
