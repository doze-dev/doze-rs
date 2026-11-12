//! What the app shows of a running kvrocks and can do to it: memory, the disk its data really takes (kvrocks's own
//! disk figures are the whole disk), keys in each namespace, the commands clients send as they send them, and three
//! actions: empty a namespace, recount keys, and compact (give deleted data's disk space back). The Go module showed
//! nothing.
//!
//! kvrocks counts keys lazily: `DBSIZE` answers the last count, and `DBSIZE SCAN` starts a new one in the background.
//! Each look starts a recount, so the next look is current; the console polls.

use crate::KvrocksSpec;
use doze_module::proto::msg::console::{
    Action, Column, ConsoleSpec, Extension, Node, Panel, Resource, Row,
};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result, help};
use doze_resp::{Client, human_bytes, parse_info, parse_monitor};
use std::collections::BTreeMap;

/// The name the app shows for the namespace the password opens.
pub(crate) const DEFAULT: &str = "(default)";

fn unavailable(e: std::io::Error) -> Error {
    Error::new(ErrorKind::Unavailable, e.to_string())
}

async fn client(cx: &Cx<KvrocksSpec>, token: Option<&str>) -> Result<Client<doze_os::net::Stream>> {
    let stream = help::dial_backend(&cx.instance).await?;
    Client::authenticated(stream, token)
        .await
        .map_err(unavailable)
}

/// A namespace's token by the name the app shows; `None` for the default namespace without a password.
fn token<'a>(spec: &'a KvrocksSpec, name: &str) -> Result<Option<&'a str>> {
    match spec.namespace.iter().find(|n| n.name == name) {
        Some(n) => Ok(Some(n.token.as_str())),
        None if name == DEFAULT || name.is_empty() => Ok(spec.password()),
        None => Err(Error::new(
            ErrorKind::NotFound,
            format!("no namespace {name:?}"),
        )),
    }
}

async fn keys(cx: &Cx<KvrocksSpec>, token: Option<&str>) -> Result<u64> {
    let mut c = client(cx, token).await?;
    let n = c.text(&["DBSIZE"]).await.map_err(unavailable)?;
    c.text(&["DBSIZE", "SCAN"]).await.map_err(unavailable)?;
    Ok(n.parse().unwrap_or(0))
}

fn namespace(name: &str, keys: u64) -> Resource {
    Resource {
        kind: "namespace".into(),
        name: name.into(),
        status: String::new(),
        info: BTreeMap::from([("keys".into(), keys.to_string())]),
    }
}

/// The server, then each namespace with its keys (the default one's are what the others leave of the total: under
/// the password `DBSIZE` counts every namespace).
pub(crate) async fn resources(cx: &Cx<KvrocksSpec>) -> Result<Vec<Resource>> {
    let text = client(cx, cx.spec.password())
        .await?
        .text(&["INFO"])
        .await
        .map_err(unavailable)?;
    let info = parse_info(&text);
    let total = keys(cx, cx.spec.password()).await?;
    let mut spaces = Vec::new();
    for ns in &cx.spec.namespace {
        spaces.push(namespace(&ns.name, keys(cx, Some(&ns.token)).await?));
    }
    let others: u64 = spaces
        .iter()
        .filter_map(|r| r.info.get("keys")?.parse::<u64>().ok())
        .sum();
    let get = |k: &str| info.get(k).cloned().unwrap_or_default();
    let memory = info
        .get("used_memory_rss")
        .and_then(|v| v.parse().ok())
        .map(human_bytes);
    let disk = human_bytes(help::dir_size(std::path::Path::new(&cx.instance.data_dir)));
    let entries = [
        ("version", get("version")),
        ("memory", memory.unwrap_or_default()),
        ("disk", disk),
        ("keys", total.to_string()),
        ("clients", get("connected_clients")),
        ("hits", get("keyspace_hits")),
        ("misses", get("keyspace_misses")),
    ];
    let server = Resource {
        kind: "server".into(),
        name: "kvrocks".into(),
        status: "up".into(),
        info: entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    };
    let mut out = vec![server, namespace(DEFAULT, total.saturating_sub(others))];
    out.extend(spaces);
    Ok(out)
}

/// Runs `flush` (one namespace's keys), `recount` (start counting keys again) or `compact` (give deleted data's disk
/// back).
pub(crate) async fn run_action(
    cx: &Cx<KvrocksSpec>,
    action: &str,
    resource: &str,
) -> Result<String> {
    match action {
        "flush" => {
            let mut c = client(cx, token(&cx.spec, resource)?).await?;
            c.text(&["FLUSHDB"]).await.map_err(unavailable)?;
            Ok(format!(
                "emptied {}",
                if resource.is_empty() {
                    DEFAULT
                } else {
                    resource
                }
            ))
        }
        "recount" | "compact" => {
            let cmd: &[&str] = if action == "recount" {
                &["DBSIZE", "SCAN"]
            } else {
                &["COMPACT"]
            };
            client(cx, cx.spec.password())
                .await?
                .text(cmd)
                .await
                .map_err(unavailable)?;
            Ok(if action == "recount" {
                "counting keys"
            } else {
                "compacting"
            }
            .into())
        }
        other => Err(Error::new(
            ErrorKind::NotFound,
            format!("kvrocks has no action {other:?}"),
        )),
    }
}

/// Streams the commands clients send (every namespace's) until the host stops reading. `MONITOR` slows the server
/// while it runs, so it runs only while someone watches.
pub(crate) async fn tail(cx: &Cx<KvrocksSpec>, out: &doze_module::caps::Rows<'_>) -> Result<()> {
    let mut c = client(cx, cx.spec.password()).await?;
    c.text(&["MONITOR"]).await.map_err(unavailable)?;
    loop {
        let line = c
            .next_reply()
            .await
            .map_err(unavailable)?
            .text()
            .unwrap_or_default();
        let Some(m) = parse_monitor(&line) else {
            continue;
        };
        let cells = BTreeMap::from([
            ("time".to_string(), m.time),
            ("client".to_string(), m.client),
            ("command".to_string(), m.command),
        ]);
        out.send(&Row {
            cells,
            tone: String::new(),
        })
        .await?;
    }
}

pub(crate) fn actions() -> Vec<Action> {
    let action = |id: &str, label: &str, kind: &str, destructive: bool| Action {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        destructive,
        input_hint: None,
    };
    vec![
        action("flush", "Empty this namespace", "namespace", true),
        action("recount", "Count keys again", "server", false),
        action(
            "compact",
            "Compact (free deleted data's disk)",
            "server",
            false,
        ),
    ]
}

fn column(key: &str, title: &str, role: &str) -> Column {
    let align = if role.is_empty() { "left" } else { "right" };
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
        select: None,
    }
}

fn block(kind: &str, panel: &str, title: &str) -> Node {
    Node {
        kind: kind.into(),
        panel: panel.into(),
        title: title.into(),
        weight: 1,
        ..Node::default()
    }
}

/// The console: the server's numbers, the namespaces, the live command stream; on the app's overview, the numbers,
/// the namespaces and the actions after its vitals card.
pub(crate) fn console() -> ConsoleSpec {
    let server = panel(
        "server",
        "Kvrocks",
        "counters",
        "server",
        vec![
            column("memory", "Memory", ""),
            column("disk", "Disk", ""),
            column("keys", "Keys", "count"),
            column("clients", "Clients", "count"),
            column("hits", "Hits", "rate"),
        ],
    );
    let namespaces = panel(
        "namespaces",
        "Namespaces",
        "table",
        "namespace",
        vec![
            column("name", "Namespace", ""),
            column("keys", "Keys", "count"),
        ],
    );
    let commands = panel(
        "commands",
        "Commands",
        "stream",
        "",
        vec![
            column("time", "Time", "time"),
            column("client", "Namespace and client", ""),
            column("command", "Command", "code"),
        ],
    );
    let overview = Node {
        kind: "row".into(),
        children: vec![
            block("stat", "server", "Kvrocks"),
            block("table", "namespaces", "Namespaces"),
            block("actions", "", "Actions"),
        ],
        ..Node::default()
    };
    ConsoleSpec {
        panels: vec![server, namespaces, commands],
        tabs: vec![],
        web_url: None,
        front: None,
        extend: vec![Extension {
            after: "vitals".into(),
            node: overview,
        }],
    }
}
