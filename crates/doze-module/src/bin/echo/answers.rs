//! The echo module's answers to the console's block calls (protocol minor 2): a value viewer over messages of each
//! kind, and a schema tree that pages and filters. The declarations are in blocks.rs.

use super::{Echo, EchoSpec};
use doze_module::caps::{Tree, Values};
use doze_module::proto::msg::blocks::{Children, TreeNode, ValueAnswer, media};
use doze_module::{Cx, Result};
use std::collections::BTreeMap;

const ORDER: &str = r#"{"id":10433,"status":"open","total":42.1,"lines":[{"sku":"A-1","qty":2},{"sku":"B-7","qty":1},{"sku":"C-3","qty":4}],"customer":{"id":2040,"name":"Ada","email":"ada@example.com","tier":"gold"}}"#;

/// Message name, what it is, its body.
pub(super) const MESSAGES: &[(&str, &str, &[u8])] = &[
    ("order-10433", media::JSON, ORDER.as_bytes()),
    (
        "note",
        media::TEXT,
        b"The night batch ran late: 3 orders were held for review and released at 06:10.",
    ),
    (
        "blob",
        media::BYTES,
        &[
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R',
            0xff, 0x00,
        ],
    ),
    ("headers", media::PAIRS, b""),
];

impl Values for Echo {
    /// The message the row names, as what it is; `headers` is pairs with a JSON body; a tree leaf, its table or its
    /// event as JSON.
    async fn value(
        &self,
        _cx: &Cx<EchoSpec>,
        _panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer> {
        use doze_module::proto::msg::error::{Error, ErrorKind};
        let name = row
            .get("name")
            .or_else(|| row.get("id"))
            .map(String::as_str)
            .unwrap_or_default();
        let name = name.rsplit('/').next().unwrap_or(name);
        if name == "headers" {
            let mut v = ValueAnswer::slice(media::PAIRS, ORDER.as_bytes(), offset, max);
            v.pairs = vec![
                (
                    "traceparent".into(),
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                ),
                ("source".into(), "api".into()),
            ];
            return Ok(v);
        }
        if let Some((_, m, body)) = MESSAGES.iter().find(|(n, _, _)| *n == name) {
            return Ok(ValueAnswer::slice(m, body, offset, max));
        }
        if let Some(rows) = TABLES.iter().find(|(t, _)| *t == name).map(|(_, r)| r) {
            let body = format!(
                "{{\"table\":\"{name}\",\"rows\":{rows},\"columns\":[\"id\",\"status\",\"total\"]}}"
            );
            return Ok(ValueAnswer::slice(
                media::JSON,
                body.as_bytes(),
                offset,
                max,
            ));
        }
        if name.starts_with("event-") {
            let body = format!("{{\"event\":\"{name}\",\"at\":\"2026-11-24T14:00:00Z\"}}");
            return Ok(ValueAnswer::slice(
                media::JSON,
                body.as_bytes(),
                offset,
                max,
            ));
        }
        Err(Error::new(
            ErrorKind::NotFound,
            format!("no message {name}"),
        ))
    }
}

/// The tables of `app.public` and their row counts.
const TABLES: &[(&str, &str)] = &[("orders", "12431"), ("customers", "2040")];

fn folder(id: &str, label: &str) -> TreeNode {
    TreeNode {
        id: id.into(),
        label: label.into(),
        ..TreeNode::default()
    }
}

fn table_leaf(id: &str, label: &str, rows: &str) -> TreeNode {
    TreeNode {
        leaf: true,
        info: BTreeMap::from([("rows".into(), rows.into())]),
        ..folder(id, label)
    }
}

/// Every node under `parent`, as a database's catalogue would list them.
fn under(parent: &str) -> Vec<TreeNode> {
    match parent {
        "" => vec![folder("app", "app"), folder("app_test", "app_test")],
        "app" => vec![folder("app/public", "public"), folder("app/audit", "audit")],
        "app/public" => TABLES
            .iter()
            .map(|(t, rows)| table_leaf(&format!("app/public/{t}"), t, rows))
            .collect(),
        "app/audit" => (0..120)
            .map(|i| TreeNode {
                leaf: true,
                ..folder(&format!("app/audit/event-{i:03}"), &format!("event-{i:03}"))
            })
            .collect(),
        _ => vec![],
    }
}

impl Tree for Echo {
    /// Pages of 50; a filter searches every folder, opened or not.
    async fn children(
        &self,
        _cx: &Cx<EchoSpec>,
        _panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> Result<Children> {
        let nodes = if filter.is_empty() {
            under(parent)
        } else {
            let mut all = Vec::new();
            let mut open = vec![parent.to_string()];
            while let Some(p) = open.pop() {
                for n in under(&p) {
                    if !n.leaf {
                        open.push(n.id.clone());
                    }
                    if n.label.contains(filter) {
                        all.push(n);
                    }
                }
            }
            all.sort_by(|a, b| a.id.cmp(&b.id));
            all
        };
        Ok(Children::page(nodes, cursor, 50))
    }
}
