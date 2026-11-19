//! The one place the daemon's wire types become the words' inputs (`doze-display`), for every surface that says them:
//! the app (`doze-desk`) and the CLI. Feature `words`.
//!
//! `doze-display` was built before `doze-control` and holds its own small types: only the fields the words read, each
//! daemon word parsed into an enum. Every conversion is here, so a field the words read cannot be dropped on one
//! surface and carried on another; doze-desk's `tests/convert.rs` checks each is carried over.

use crate as wire;
use doze_display as words;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An instance, for the words.
pub fn instance(v: &wire::InstanceView) -> words::Instance {
    words::Instance {
        name: v.name.clone(),
        kind: words::Kind::parse(&v.kind),
        task: v.task.as_ref().map(task),
        run: v.run.clone(),
        needs: v.needs.clone(),
        engine: v.engine.clone(),
        state: words::State::parse(&v.state),
        version: v.version.clone(),
        holders: v.holders.iter().map(holder).collect(),
        started_at: time(&v.started_at),
        idle_since: time(&v.idle_since),
        idle_after: v.idle_after.to_std(),
        always_on: v.always_on,
        ram: u64::try_from(v.ram).unwrap_or(0),
        endpoint: v.endpoint.clone(),
        bind: v.bind.clone(),
        domain: v.domain.clone(),
        name_only: v.name_only,
        console: v.console.clone(),
        has_console: v.has_console,
        console_front: v.console_front.clone(),
        web_url: v.web_url.clone(),
        name_held: v.name_held.clone(),
        url: v.url.clone(),
        resource: v.resource.clone(),
        data_dir: v.data_dir.clone(),
        last_error: v.last_error.clone(),
        tainted: v.tainted,
        disabled: v.disabled,
        paused: v.paused,
        keep_awake: v.keep_awake,
        restart_count: count(v.restart_count),
        restart_max: count(v.restart_max),
        next_restart: time(&v.next_restart),
    }
}

/// A stack's instances and idle time, for the words.
pub fn snapshot(idle_timeout: Duration, instances: &[wire::InstanceView]) -> words::Snapshot {
    words::Snapshot {
        idle_timeout,
        instances: instances.iter().map(instance).collect(),
    }
}

/// How a task last ran.
pub fn task(t: &wire::TaskView) -> words::TaskRecord {
    words::TaskRecord {
        outcome: words::Outcome::parse(&t.result),
        exit_code: t.exit_code,
        took: Duration::from_millis(u64::try_from(t.took_ms).unwrap_or(0)),
        ran_at: time(&t.ran_at),
        why: t.why.clone(),
    }
}

/// One reason an instance is awake.
pub fn holder(h: &wire::HolderView) -> words::Holder {
    match h.kind.as_str() {
        "dependant" => words::Holder::Dependant(h.name.clone()),
        "connection" => words::Holder::Connections(count(h.count)),
        "pin" => words::Holder::Pin,
        _ => words::Holder::Other,
    }
}

/// A journal event. The words need a time; an event the daemon sent with none reads as the epoch.
pub fn event(e: &wire::Event) -> words::Event {
    words::Event {
        time: time(&e.time).unwrap_or(UNIX_EPOCH),
        kind: words::EventKind::parse(&e.kind),
        instance: e.instance.clone(),
        cause: words::Cause::parse(&e.cause.kind, &e.cause.by),
        took: e.took.to_std(),
        detail: e.detail.clone(),
    }
}

/// A module's console spec: its panels and its tabs (what the words lay out and write cells from).
pub fn console_spec(s: &wire::ConsoleSpec) -> words::ConsoleSpec {
    words::ConsoleSpec {
        panels: s.panels.iter().map(panel).collect(),
        tabs: s.tabs.iter().map(console_tab).collect(),
    }
}

/// One tab of a console.
pub fn console_tab(t: &wire::ConsoleTab) -> words::ConsoleTab {
    words::ConsoleTab {
        id: t.id.clone(),
        title: t.title.clone(),
        body: node(&t.body),
    }
}

/// A node of a console layout.
pub fn node(n: &wire::ConsoleNode) -> words::Node {
    words::Node {
        kind: words::NodeKind::parse(&n.kind),
        id: n.id.clone(),
        title: n.title.clone(),
        weight: count(n.weight),
        panel: n.panel.clone(),
        from: n.from.clone(),
        children: n.children.iter().map(node).collect(),
    }
}

/// A panel of a console.
pub fn panel(p: &wire::ConsolePanel) -> words::Panel {
    words::Panel {
        id: p.id.clone(),
        title: p.title.clone(),
        kind: words::PanelKind::parse(&p.kind),
        columns: p.columns.iter().map(column).collect(),
        select: p.select.clone(),
    }
}

/// A column of a console table.
pub fn column(c: &wire::ConsoleColumn) -> words::Column {
    words::Column {
        key: c.key.clone(),
        title: c.title.clone(),
        role: words::Role::parse(&c.role),
    }
}

/// A console row. Go's `"cells":null` is a row with no cells.
pub fn console_row(r: &wire::ConsoleRow) -> words::ConsoleRow {
    words::ConsoleRow {
        cells: r
            .cells
            .as_ref()
            .map(|c| c.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default(),
        tone: r.tone.clone(),
    }
}

/// A time the daemon sent, or `None` for Go's zero time.
pub fn time(t: &wire::Timestamp) -> Option<SystemTime> {
    t.to_system_time()
}

/// A count the daemon sends as a signed number; a negative one is zero.
fn count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}
