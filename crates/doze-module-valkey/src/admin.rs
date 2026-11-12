//! What the app shows of a running valkey and can do to it: memory against its ceiling, keys, clients, hits and
//! evictions (from `INFO`), each database's keys, the commands clients send as they send them (`MONITOR`), and two
//! actions, empty the cache and save a snapshot now. The Go module showed nothing.

use crate::ValkeySpec;
use doze_module::proto::msg::console::{
    Action, Column, ConsoleSpec, Extension, Node, Panel, Resource, Row,
};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result, help};
use doze_resp::{Client, fields, human_bytes, parse_info, parse_monitor};
use std::collections::BTreeMap;

fn unavailable(e: std::io::Error) -> Error {
    Error::new(ErrorKind::Unavailable, e.to_string())
}

async fn client(cx: &Cx<ValkeySpec>) -> Result<Client<doze_os::net::Stream>> {
    let stream = help::dial_backend(&cx.instance).await?;
    Client::authenticated(stream, cx.spec.password())
        .await
        .map_err(unavailable)
}

fn bytes(info: &BTreeMap<String, String>, k: &str) -> String {
    info.get(k)
        .and_then(|v| v.parse().ok())
        .map(human_bytes)
        .unwrap_or_default()
}

fn persistence(spec: &ValkeySpec) -> &'static str {
    match (spec.appendonly, spec.save.as_deref()) {
        (true, _) => "append-only file",
        (false, None) => "snapshot when it sleeps",
        (false, Some("")) => "none: memory only",
        (false, Some(_)) => "snapshots on a schedule",
    }
}

fn server(info: &BTreeMap<String, String>, spec: &ValkeySpec) -> Resource {
    let get = |k: &str| info.get(k).cloned().unwrap_or_default();
    let keys: u64 = info
        .iter()
        .filter(|(k, _)| k.starts_with("db"))
        .filter_map(|(_, v)| fields(v).get("keys").and_then(|n| n.parse::<u64>().ok()))
        .sum();
    let ceiling = match get("maxmemory").as_str() {
        "0" | "" => "no limit".to_string(),
        _ => bytes(info, "maxmemory"),
    };
    let entries = [
        ("version", get("valkey_version")),
        ("memory", bytes(info, "used_memory")),
        ("ceiling", ceiling),
        ("policy", get("maxmemory_policy")),
        ("keys", keys.to_string()),
        ("clients", get("connected_clients")),
        ("hits", get("keyspace_hits")),
        ("misses", get("keyspace_misses")),
        ("evicted", get("evicted_keys")),
        ("persistence", persistence(spec).to_string()),
    ];
    Resource {
        kind: "server".into(),
        name: "valkey".into(),
        status: "up".into(),
        info: entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

/// The server, then one row per database that holds keys.
pub(crate) async fn resources(cx: &Cx<ValkeySpec>) -> Result<Vec<Resource>> {
    let text = client(cx)
        .await?
        .text(&["INFO"])
        .await
        .map_err(unavailable)?;
    let info = parse_info(&text);
    let mut out = vec![server(&info, &cx.spec)];
    for (db, v) in info.iter().filter(|(k, _)| k.starts_with("db")) {
        let f = fields(v);
        let at = |k: &str| f.get(k).cloned().unwrap_or_default();
        out.push(Resource {
            kind: "database".into(),
            name: db.clone(),
            status: String::new(),
            info: BTreeMap::from([
                ("keys".into(), at("keys")),
                ("expiring".into(), at("expires")),
            ]),
        });
    }
    Ok(out)
}

/// Runs `flush` (every key, every database) or `save` (a snapshot, in the background).
pub(crate) async fn run_action(cx: &Cx<ValkeySpec>, action: &str) -> Result<String> {
    let (cmd, done) = match action {
        "flush" => ("FLUSHALL", "emptied"),
        "save" => ("BGSAVE", "saving a snapshot"),
        other => {
            let e = format!("valkey has no action {other:?}");
            return Err(Error::new(ErrorKind::NotFound, e));
        }
    };
    client(cx).await?.text(&[cmd]).await.map_err(unavailable)?;
    Ok(done.into())
}

/// Streams the commands clients send until the host stops reading. `MONITOR` slows the server while it runs, so it
/// runs only while someone watches.
pub(crate) async fn tail(cx: &Cx<ValkeySpec>, out: &doze_module::caps::Rows<'_>) -> Result<()> {
    let mut c = client(cx).await?;
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
    let action = |id: &str, label: &str, destructive: bool| Action {
        id: id.into(),
        label: label.into(),
        kind: "server".into(),
        destructive,
        input_hint: None,
    };
    vec![
        action("flush", "Empty the cache", true),
        action("save", "Save a snapshot now", false),
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

/// The console: the server's numbers and the databases, the live command stream, and on the app's overview the
/// numbers and the actions after its vitals card.
pub(crate) fn console() -> ConsoleSpec {
    let server = panel(
        "server",
        "Valkey",
        "counters",
        "server",
        vec![
            column("memory", "Memory", ""),
            column("ceiling", "Ceiling", ""),
            column("keys", "Keys", "count"),
            column("clients", "Clients", "count"),
            column("hits", "Hits", "rate"),
            column("evicted", "Evictions", "rate"),
            column("persistence", "Keeps data", ""),
        ],
    );
    let databases = panel(
        "databases",
        "Databases",
        "table",
        "database",
        vec![
            column("name", "Database", ""),
            column("keys", "Keys", "count"),
            column("expiring", "Expiring", "count"),
        ],
    );
    let commands = panel(
        "commands",
        "Commands",
        "stream",
        "",
        vec![
            column("time", "Time", "time"),
            column("client", "Client", ""),
            column("command", "Command", "code"),
        ],
    );
    let overview = Node {
        kind: "row".into(),
        children: vec![
            block("stat", "server", "Valkey"),
            block("actions", "", "Actions"),
        ],
        ..Node::default()
    };
    ConsoleSpec {
        panels: vec![server, databases, commands],
        tabs: vec![],
        web_url: None,
        front: None,
        extend: vec![Extension {
            after: "vitals".into(),
            node: overview,
        }],
    }
}
