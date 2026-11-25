//! The console's tabs: "Console", the layout an app drew from the panels alone before (the tables side by side, the
//! tail of the selected topic below, then the actions), and "Browse", the topics as a tree beside the message opened
//! in it.

use doze_module::proto::msg::blocks::kind;
use doze_module::proto::msg::console::{ConsoleSpec, Node, Panel, Tab};

use super::{admin, browse};

fn block(kind: &str, id: &str, panel: &str) -> Node {
    Node {
        kind: kind.into(),
        id: id.into(),
        panel: panel.into(),
        ..Node::default()
    }
}

fn tab(id: &str, title: &str, children: Vec<Node>) -> Tab {
    Tab {
        id: id.into(),
        title: title.into(),
        body: Node {
            kind: "col".into(),
            children,
            ..Node::default()
        },
    }
}

fn row(children: Vec<Node>) -> Node {
    Node {
        kind: "row".into(),
        weight: 1,
        children,
        ..Node::default()
    }
}

/// The console: the Go module's panels and web console, the tree's panel, and the two tabs.
pub(crate) fn console() -> ConsoleSpec {
    let mut c = admin::console();
    c.panels.push(Panel {
        id: browse::PANEL.into(),
        title: "Topics".into(),
        kind: kind::TREE.into(),
        ..Panel::default()
    });
    let tail = Node {
        from: "topics".into(),
        ..block("stream", "tail", "tail")
    };
    let tree = Node {
        title: "Topics".into(),
        weight: 1,
        ..block(kind::TREE, "browse", browse::PANEL)
    };
    let message = Node {
        title: "Message".into(),
        weight: 2,
        from: "browse".into(),
        ..block(kind::VALUE, "message", browse::PANEL)
    };
    c.tabs = vec![
        tab(
            "console",
            "Console",
            vec![
                row(vec![
                    block("table", "topics", "topics"),
                    block("table", "groups", "groups"),
                ]),
                tail,
                block("actions", "", ""),
            ],
        ),
        tab("browse", "Browse", vec![row(vec![tree, message])]),
    ];
    c
}
