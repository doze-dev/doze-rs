//! The console: what the server is doing now (counts, sessions and their queries, databases and their size), and two
//! actions on a session (cancel its query, end it). Read with psql over the socket, as Go did.

use crate::Postgres;
use crate::psql::{self, Psql};
use crate::spec::PostgresSpec;
use doze_module::caps::Admin;
use doze_module::proto::msg::console::{
    Action, Column, ConsoleSpec, Extension, Node, Panel, Resource, Tab,
};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};
use std::collections::BTreeMap;

const ACTIVITY: &str = "
SELECT count(*) FILTER (WHERE state = 'active'),
       count(*) FILTER (WHERE state LIKE 'idle%'),
       count(*) FILTER (WHERE wait_event_type = 'Lock'),
       count(*),
       current_setting('max_connections'),
       coalesce(round(max(extract(epoch FROM now() - query_start))::numeric, 1), 0)
FROM pg_stat_activity WHERE backend_type = 'client backend'";

const SESSIONS: &str = r"
SELECT pid, usename, datname, coalesce(nullif(application_name, ''), '-'),
       coalesce(round(extract(epoch FROM now() - state_change)::numeric, 1), 0),
       CASE WHEN wait_event_type = 'Lock' THEN 'waiting' ELSE coalesce(state, '') END,
       left(regexp_replace(regexp_replace(coalesce(query, ''), '\s+', ' ', 'g'), '\x1f', ' ', 'g'), 600)
FROM pg_stat_activity
WHERE backend_type = 'client backend' AND pid <> pg_backend_pid()
ORDER BY CASE WHEN wait_event_type = 'Lock' THEN 0 WHEN state = 'active' THEN 1 ELSE 2 END, state_change DESC
LIMIT 200";

/// The databases, but the server's own (`postgres`), unless it is the instance's: an instance may be named after it.
fn databases(instance: &str) -> String {
    format!(
        "SELECT d.datname, pg_database_size(d.datname),
       (SELECT count(*) FROM pg_stat_activity a WHERE a.datname = d.datname), pg_get_userbyid(d.datdba)
FROM pg_database d WHERE NOT d.datistemplate AND (d.datname <> 'postgres' OR d.datname = {}) ORDER BY 1",
        crate::sql::lit(instance)
    )
}

fn info(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn resource(kind: &str, name: &str, status: &str, pairs: &[(&str, &str)]) -> Resource {
    Resource {
        kind: kind.into(),
        name: name.into(),
        status: status.into(),
        info: info(pairs),
    }
}

async fn resources(p: &Psql, instance: &str) -> Result<Vec<Resource>> {
    let mut out = Vec::new();
    for r in p.rows("postgres", ACTIVITY, 6).await? {
        let [active, idle, waiting, connections, of, longest] = r.as_slice() else {
            continue;
        };
        let longest = format!("{longest} s");
        let pairs = [
            ("active", active.as_str()),
            ("idle", idle),
            ("waiting", waiting),
            ("connections", connections),
            ("of", of),
            ("longest", &longest),
        ];
        out.push(resource("activity", "now", "", &pairs));
    }
    for r in p.rows("postgres", SESSIONS, 7).await? {
        let [pid, user, db, app, secs, state, query] = r.as_slice() else {
            continue;
        };
        let since = format!("{secs} s");
        let pairs = [
            ("pid", pid.as_str()),
            ("user", user),
            ("database", db),
            ("app", app),
            ("for", &since),
            ("state", state),
            ("query", query),
        ];
        out.push(resource("session", pid, state, &pairs));
    }
    for r in p.rows("postgres", &databases(instance), 4).await? {
        let [name, size, conns, owner] = r.as_slice() else {
            continue;
        };
        let pairs = [
            ("database", name.as_str()),
            ("size", size),
            ("connections", conns),
            ("owner", owner),
        ];
        out.push(resource("database", name, "", &pairs));
    }
    Ok(out)
}

impl Admin for Postgres {
    async fn resources(&self, cx: &Cx<PostgresSpec>) -> Result<Vec<Resource>> {
        resources(&psql::admin(&cx.instance)?, &cx.instance.name).await
    }

    async fn run_action(
        &self,
        cx: &Cx<PostgresSpec>,
        action: &str,
        resource: &str,
        _input: Option<&str>,
    ) -> Result<String> {
        let pid = resource.trim().parse::<u32>().ok().filter(|p| *p > 0);
        let Some(pid) = pid else {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!("{resource:?} is not a session"),
            ));
        };
        let (func, done) = match action {
            "cancel" => (
                "pg_cancel_backend",
                format!("cancelled the query of session {pid}"),
            ),
            "terminate" => ("pg_terminate_backend", format!("ended session {pid}")),
            other => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("postgres has no action {other:?}"),
                ));
            }
        };
        let p = psql::admin(&cx.instance)?;
        let got = p
            .column("postgres", &format!("SELECT {func}({pid})"))
            .await?;
        if got.first().is_some_and(|v| v == "f") {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("session {pid} is already gone"),
            ));
        }
        Ok(done)
    }
}

pub(crate) fn actions() -> Vec<Action> {
    let action = |id: &str, label: &str, destructive: bool| Action {
        id: id.into(),
        label: label.into(),
        kind: "session".into(),
        destructive,
        input_hint: None,
    };
    vec![
        action("cancel", "Cancel query", false),
        action("terminate", "Terminate session", true),
    ]
}

fn column(key: &str, title: &str, role: &str, right: bool) -> Column {
    let align = if right { "right" } else { "left" };
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

fn node(kind: &str, panel: &str, title: &str, weight: u32, children: Vec<Node>) -> Node {
    Node {
        kind: kind.into(),
        panel: panel.into(),
        title: title.into(),
        weight,
        children,
        ..Node::default()
    }
}

/// The data the console draws.
fn panels() -> Vec<Panel> {
    let activity = panel(
        "activity",
        "Activity",
        "counters",
        "activity",
        vec![
            column("active", "Active", "count", false),
            column("idle", "Idle", "count", false),
            column("waiting", "Waiting", "count", false),
            column("connections", "Connections", "count", false),
            column("of", "Of", "of", false),
            column("longest", "Longest", "", false),
        ],
    );
    let sessions = panel(
        "sessions",
        "Sessions",
        "table",
        "session",
        vec![
            column("pid", "Pid", "", true),
            column("user", "User", "", false),
            column("database", "Database", "", false),
            column("app", "App", "", false),
            column("for", "For", "", true),
            column("state", "State", "state", false),
            column("query", "Query", "detail", false),
        ],
    );
    let databases = panel(
        "databases",
        "Databases",
        "table",
        "database",
        vec![
            column("database", "Database", "", false),
            column("size", "Size", "bytes", true),
            column("connections", "Connections", "count", true),
            column("owner", "Owner", "", false),
        ],
    );
    vec![activity, sessions, databases]
}

/// Go's layout: an activity tab (the counts, the sessions beside the selected one's query and the actions), and the
/// databases after the overview's vitals card.
pub(crate) fn console() -> ConsoleSpec {
    let query = Node {
        from: "sessions".into(),
        ..node("detail", "", "query", 1, vec![])
    };
    let side = node(
        "col",
        "",
        "",
        1,
        vec![query, node("actions", "", "", 1, vec![])],
    );
    let body = node(
        "col",
        "",
        "",
        1,
        vec![
            node("stat", "activity", "", 1, vec![]),
            node(
                "row",
                "",
                "",
                1,
                vec![node("table", "sessions", "", 2, vec![]), side],
            ),
        ],
    );
    ConsoleSpec {
        panels: panels(),
        tabs: vec![Tab {
            id: "activity".into(),
            title: "Activity".into(),
            body,
        }],
        web_url: None,
        front: Some("overview".into()),
        extend: vec![Extension {
            after: "vitals".into(),
            node: node("table", "databases", "", 1, vec![]),
        }],
    }
}

/// The console and actions, pinned: the app's `consoles` fixture is generated from `console.snapshot.json`, never
/// written by hand. `DOZE_UPDATE_SNAPSHOT=1` rewrites it after a deliberate change.
#[cfg(test)]
mod snapshot {
    #[test]
    fn the_console_and_actions_match_their_snapshot() {
        let now =
            doze_module_proto::msg::snapshot::console_json(&super::console(), &super::actions());
        let pinned = doze_module_proto::msg::snapshot::pin(
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/console.snapshot.json"),
            &now,
        );
        assert_eq!(pinned, Ok(()));
    }
}
