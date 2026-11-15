//! The oracle's inputs, read back into this crate's types the way the protocol crate will read the daemon's: each
//! word parsed once.

use std::time::Duration;

use doze_display::{
    Cause, Event, EventKind, Holder, Instance, Kind, Outcome, Snapshot, State, TaskRecord,
};
use serde::Deserialize;

use crate::time;

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct TaskIn {
    result: String,
    exit: i64,
    took_ms: u64,
    ran_at: Option<i64>,
    why: String,
}

impl TaskIn {
    pub(crate) fn record(&self) -> TaskRecord {
        TaskRecord {
            outcome: Outcome::parse(&self.result),
            exit_code: self.exit,
            took: Duration::from_millis(self.took_ms),
            ran_at: self.ran_at.map(time),
            why: self.why.clone(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct HolderIn {
    kind: String,
    name: String,
    count: u32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct InstIn {
    name: String,
    kind: String,
    run: String,
    engine: String,
    state: String,
    version: String,
    task: Option<TaskIn>,
    needs: Vec<String>,
    holders: Vec<HolderIn>,
    started_at: Option<i64>,
    idle_since: Option<i64>,
    next_restart: Option<i64>,
    idle_after: u64,
    always_on: bool,
    name_only: bool,
    has_console: bool,
    tainted: bool,
    disabled: bool,
    paused: bool,
    keep_awake: bool,
    ram: u64,
    endpoint: String,
    bind: String,
    domain: String,
    console: String,
    console_front: String,
    web_url: String,
    name_held: String,
    url: String,
    resource: String,
    data_dir: String,
    last_error: String,
    restart_count: u32,
    restart_max: u32,
}

impl InstIn {
    pub(crate) fn instance(self) -> Instance {
        let holders = self
            .holders
            .into_iter()
            .map(|h| match h.kind.as_str() {
                "dependant" => Holder::Dependant(h.name),
                "connection" => Holder::Connections(h.count),
                "pin" => Holder::Pin,
                _ => Holder::Other,
            })
            .collect();
        Instance {
            kind: Kind::parse(&self.kind),
            state: State::parse(&self.state),
            task: self.task.as_ref().map(TaskIn::record),
            holders,
            started_at: self.started_at.map(time),
            idle_since: self.idle_since.map(time),
            next_restart: self.next_restart.map(time),
            idle_after: Duration::from_nanos(self.idle_after),
            name: self.name,
            run: self.run,
            needs: self.needs,
            engine: self.engine,
            version: self.version,
            always_on: self.always_on,
            ram: self.ram,
            endpoint: self.endpoint,
            bind: self.bind,
            domain: self.domain,
            name_only: self.name_only,
            console: self.console,
            has_console: self.has_console,
            console_front: self.console_front,
            web_url: self.web_url,
            name_held: self.name_held,
            url: self.url,
            resource: self.resource,
            data_dir: self.data_dir,
            last_error: self.last_error,
            tainted: self.tainted,
            disabled: self.disabled,
            paused: self.paused,
            keep_awake: self.keep_awake,
            restart_count: self.restart_count,
            restart_max: self.restart_max,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct SnapIn {
    idle: u64,
    instances: Vec<InstIn>,
}

impl SnapIn {
    pub(crate) fn snapshot(self) -> Snapshot {
        Snapshot {
            idle_timeout: Duration::from_nanos(self.idle),
            instances: self.instances.into_iter().map(InstIn::instance).collect(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct EventIn {
    time: i64,
    kind: String,
    instance: String,
    cause: String,
    by: String,
    took: u64,
    detail: String,
}

impl EventIn {
    pub(crate) fn event(&self) -> Event {
        Event {
            time: time(self.time),
            kind: EventKind::parse(&self.kind),
            instance: self.instance.clone(),
            cause: Cause::parse(&self.cause, &self.by),
            took: Duration::from_nanos(self.took),
            detail: self.detail.clone(),
        }
    }
}
