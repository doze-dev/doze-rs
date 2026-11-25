//! The echo module's console, with every block protocol minor 2 adds, over canned but realistic data, so the app has
//! something to draw before a real module answers: a value viewer over messages of each kind, a form
//! that creates a channel, lag and connections over time, checks with a fix, and a schema tree that opens as you go.
//! This file declares them and lists their resources; answers.rs answers the calls.

use super::answers::MESSAGES;
use doze_module::Result;
use doze_module::caps::check;
use doze_module::proto::msg::blocks::check as checks;
use doze_module::proto::msg::blocks::{Field, field, kind};
use doze_module::proto::msg::console::{
    Action, Column, ConsoleSpec, Extension, Node, Panel, Resource, Tab,
};
use std::collections::BTreeMap;

fn col(key: &str, title: &str, role: &str, align: &str) -> Column {
    Column {
        key: key.into(),
        title: title.into(),
        role: role.into(),
        align: align.into(),
    }
}

fn panel(id: &str, title: &str, kind: &str, source: &str, columns: Vec<Column>) -> Panel {
    Panel {
        id: id.into(),
        title: title.into(),
        kind: kind.into(),
        source: source.into(),
        columns,
        ..Panel::default()
    }
}

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

/// The console: the channels' and the server's counters first (and the channels' stat on the app's overview), then one
/// tab per block.
pub(super) fn console() -> ConsoleSpec {
    let panels = vec![
        panel(
            "channels",
            "Channels",
            "counters",
            "channel",
            vec![col("size", "Size", "count", "right")],
        ),
        panel(
            "messages",
            "Messages",
            "table",
            "message",
            vec![
                col("name", "Message", "", "left"),
                col("media", "Kind", "", "left"),
                col("bytes", "Size", "bytes", "right"),
            ],
        ),
        panel(
            "groups",
            "Consumer groups",
            "table",
            "group",
            vec![
                col("name", "Group", "", "left"),
                col("lag", "Lag", "lag", "right"),
            ],
        ),
        panel(
            "server",
            "Server",
            "counters",
            "server",
            vec![
                col("connections", "Connections", "count", "right"),
                col("max_connections", "Limit", "of", "right"),
            ],
        ),
        panel("health", "Checks", kind::CHECKS, checks::KIND, vec![]),
        panel("browse", "Schemas", kind::TREE, "", vec![]),
    ];
    ConsoleSpec { panels, ..layout() }
}

/// The tabs, one per block, and the stat on the app's overview.
fn layout() -> ConsoleSpec {
    let value = Node {
        from: "messages".into(),
        title: "Value".into(),
        ..block(kind::VALUE, "value", "messages")
    };
    let lag = Node {
        title: "Consumer lag".into(),
        keys: vec!["lag".into()],
        by: "name".into(),
        ..block(kind::SERIES, "lag", "groups")
    };
    let conns = Node {
        title: "Connections".into(),
        keys: vec!["connections".into()],
        ..block(kind::SERIES, "conns", "server")
    };
    let tabs = vec![
        tab(
            "overview",
            "Overview",
            vec![
                block("stat", "channels", "channels"),
                block("stat", "server", "server"),
            ],
        ),
        tab(
            "messages",
            "Messages",
            vec![block("table", "messages", "messages"), value],
        ),
        tab("charts", "Charts", vec![lag, conns]),
        tab(
            "health",
            "Checks",
            vec![block(kind::CHECKS, "health", "health")],
        ),
        tab(
            "browse",
            "Browse",
            vec![block(kind::TREE, "browse", "browse")],
        ),
    ];
    let stat = Node {
        title: "Channels".into(),
        ..block("stat", "", "channels")
    };
    ConsoleSpec {
        panels: vec![],
        tabs,
        web_url: None,
        front: Some("console".into()),
        extend: vec![Extension {
            after: "vitals".into(),
            node: stat,
        }],
    }
}

/// The data actions: purge as before, a form that creates a channel, and the fix a failing check offers.
pub(super) fn actions() -> Vec<Action> {
    let f = |id: &str, label: &str, kind: &str, required: bool| Field {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        required,
        ..Field::default()
    };
    let create = Action {
        id: "create".into(),
        label: "Create a channel".into(),
        kind: "channel".into(),
        destructive: false,
        input_hint: Some("name".into()),
        fields: vec![
            f("name", "Name", field::TEXT, true),
            Field {
                default: Some("1".into()),
                min: Some(1),
                max: Some(1024),
                help: Some("Messages the channel holds.".into()),
                ..f("size", "Size", field::NUMBER, true)
            },
            Field {
                default: Some("delete".into()),
                choices: vec!["delete".into(), "compact".into()],
                ..f("cleanup", "Clean-up", field::CHOICE, false)
            },
            Field {
                default: Some("true".into()),
                ..f("keep", "Keep in doze.hcl", field::BOOL, false)
            },
            f("headers", "Headers", field::PAIRS, false),
            f("token", "Token", field::SECRET, false),
        ],
    };
    let purge = Action {
        id: "purge".into(),
        label: "Purge".into(),
        kind: "channel".into(),
        destructive: true,
        ..Action::default()
    };
    let migrate = Action {
        id: "apply-migrations".into(),
        label: "Apply migrations".into(),
        kind: checks::KIND.into(),
        ..Action::default()
    };
    vec![purge, create, migrate]
}

fn res(kind: &str, name: &str, info: &[(&str, String)]) -> Resource {
    Resource {
        kind: kind.into(),
        name: name.into(),
        status: String::new(),
        info: info
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}

/// Seconds since the epoch: readings that move, as a real engine's do.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The messages, groups, server and checks the blocks draw.
pub(super) fn resources() -> Vec<Resource> {
    let t = now() / 15;
    let mut out: Vec<Resource> = MESSAGES
        .iter()
        .map(|(name, m, body)| {
            res(
                "message",
                name,
                &[("media", (*m).into()), ("bytes", body.len().to_string())],
            )
        })
        .collect();
    out.push(res("group", "worker", &[("lag", (t % 40).to_string())]));
    out.push(res(
        "group",
        "mailer",
        &[("lag", ((t * 7) % 29).to_string())],
    ));
    out.push(res(
        "server",
        "echo",
        &[
            ("connections", (40 + t % 120).to_string()),
            ("max_connections", "200".into()),
        ],
    ));
    out.push(check(
        "disk",
        checks::OK,
        "12% of the data folder's limit",
        None,
    ));
    out.push(check(
        "maxmemory",
        checks::WARN,
        "no maxmemory set: the engine may take all the memory it wants",
        None,
    ));
    out.push(check(
        "migrations",
        checks::BAD,
        "2 migrations pending from migrations/",
        Some("apply-migrations"),
    ));
    out
}

/// What a form's run does: says what it made, never the secret.
pub(super) fn run_form(action: &str, values: &BTreeMap<String, String>) -> Result<String> {
    let v = |k: &str| values.get(k).map(String::as_str).unwrap_or_default();
    match action {
        "create" => Ok(format!(
            "created channel {} (size {}, {}{}, {} headers)",
            v("name"),
            v("size"),
            v("cleanup"),
            if v("keep") == "true" {
                ", kept in doze.hcl"
            } else {
                ""
            },
            doze_module::forms::pairs(v("headers")).len()
        )),
        "apply-migrations" => Ok("applied 2 migrations".into()),
        other => Ok(format!("{other}d")),
    }
}
