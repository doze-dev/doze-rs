//! The console layout valkey and kvrocks share: a "Console" tab with what they always showed (the server's numbers
//! beside a table, the live commands below, then the actions; what the app drew for a spec without tabs), and a
//! "Keys" tab with the key tree beside the value of the key opened in it.

use crate::browse::PANEL;
use doze_module_proto::msg::blocks::kind;
use doze_module_proto::msg::console::{Node, Panel, Tab};

fn block(kind: &str, panel: &str, title: &str) -> Node {
    Node {
        kind: kind.into(),
        panel: panel.into(),
        title: title.into(),
        weight: 1,
        ..Node::default()
    }
}

/// The "Console" tab: the counters panel `server` (titled `server_title`) beside the table panel `table`, the
/// stream panel `commands` below, then the actions.
pub fn console_tab(server_title: &str, table: &str, table_title: &str) -> Tab {
    let top = Node {
        kind: "row".into(),
        weight: 1,
        children: vec![
            block("stat", "server", server_title),
            block("table", table, table_title),
        ],
        ..Node::default()
    };
    let actions = Node {
        kind: "actions".into(),
        ..Node::default()
    };
    Tab {
        id: "console".into(),
        title: "Console".into(),
        body: Node {
            kind: "col".into(),
            children: vec![top, block("stream", "commands", "Commands"), actions],
            ..Node::default()
        },
    }
}

/// The panel the tree draws (its data comes from `children`, not from resources).
pub fn keys_panel() -> Panel {
    Panel {
        id: PANEL.into(),
        title: "Keys".into(),
        kind: kind::TREE.into(),
        ..Panel::default()
    }
}

/// The "Keys" tab: the tree beside the value of the key opened in it.
pub fn keys_tab() -> Tab {
    let tree = Node {
        kind: kind::TREE.into(),
        id: PANEL.into(),
        panel: PANEL.into(),
        title: "Keys".into(),
        weight: 1,
        ..Node::default()
    };
    let value = Node {
        kind: kind::VALUE.into(),
        id: "value".into(),
        panel: PANEL.into(),
        from: PANEL.into(),
        title: "Value".into(),
        weight: 2,
        ..Node::default()
    };
    Tab {
        id: "keys".into(),
        title: "Keys".into(),
        body: Node {
            kind: "row".into(),
            children: vec![tree, value],
            ..Node::default()
        },
    }
}
