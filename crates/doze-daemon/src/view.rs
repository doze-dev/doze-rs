//! An instance as the wire carries it (Go's `handler.view` and `snapshot`): what the engine knows (state, pid, conns,
//! holders, times, restarts, a task's last run) with what the config says ([`Labels`]).

use doze_control::{GoDuration, HolderView, InstanceView, TaskView, Timestamp};
use doze_engine::applied::TaskRecord;
use doze_engine::{Engine, Holder, Instance};

/// What the config says about one instance, for the view (the engine does not know these).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labels {
    /// `service`, `task`, or a module kind.
    pub kind: String,
    /// The engine (module) name: `postgres`, `app`, ….
    pub engine: String,
    pub version: String,
    /// The command line an app runs, as words for people.
    pub run: String,
    pub needs: Vec<String>,
    /// Where clients reach it (`host:port`).
    pub endpoint: String,
    pub domain: String,
    pub url: String,
    pub env_var: String,
    /// `enabled = false` in the config.
    pub paused: bool,
    pub group: String,
}

/// Every declared instance, in the engine's (name) order.
pub(crate) fn snapshot(
    e: &Engine,
    labels: &std::collections::BTreeMap<String, Labels>,
) -> Vec<InstanceView> {
    e.snapshot()
        .iter()
        .map(|i| view(e, i, labels.get(&i.name)))
        .collect()
}

/// One instance.
pub(crate) fn view(e: &Engine, i: &Instance, l: Option<&Labels>) -> InstanceView {
    let l = l.cloned().unwrap_or_default();
    let task = e.is_task(&i.name).then(|| e.task_record(&i.name)).flatten();
    let disabled = i.off || l.paused;
    InstanceView {
        name: i.name.clone(),
        kind: l.kind,
        task: task.map(|r| task_view(&r)),
        run: l.run,
        needs: l.needs,
        engine: l.engine,
        state: if disabled && !i.state.running() {
            "disabled".into()
        } else {
            i.state.as_str().into()
        },
        version: l.version,
        pid: i64::from(i.pid),
        conns: i64::from(i.conns),
        holders: e.holders(&i.name).iter().map(holder_view).collect(),
        started_at: stamp(i.started_at),
        idle_since: stamp(i.idle_since),
        idle_after: i.idle_after.map_or(GoDuration(0), go_duration),
        always_on: i.always_on,
        endpoint: l.endpoint,
        domain: l.domain,
        url: l.url,
        env_var: l.env_var,
        data_dir: e
            .stack()
            .cluster_dir(&i.name)
            .to_string_lossy()
            .into_owned(),
        last_error: i.last_error.clone(),
        tainted: i.tainted,
        declared: true,
        disabled,
        paused: l.paused,
        keep_awake: i.keep_awake,
        group: l.group,
        restart_count: i64::from(i.restart_count),
        restart_max: i64::from(i.restart_max),
        next_restart: stamp(i.next_restart),
        healthy: i.healthy,
        ..InstanceView::default()
    }
}

fn stamp(t: Option<std::time::SystemTime>) -> Timestamp {
    t.map_or_else(Timestamp::zero, Timestamp::from_system_time)
}

pub(crate) fn go_duration(d: std::time::Duration) -> GoDuration {
    GoDuration(i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

fn holder_view(h: &Holder) -> HolderView {
    let (name, count) = match h {
        Holder::Dependant(n) => (n.clone(), 0),
        Holder::Pin => (String::new(), 0),
        Holder::Connections(c) => (String::new(), i64::from(*c)),
    };
    HolderView {
        kind: h.kind().into(),
        name,
        count,
    }
}

fn task_view(r: &TaskRecord) -> TaskView {
    TaskView {
        result: r.result.clone(),
        exit_code: i64::from(r.exit_code),
        took_ms: r.took_ms,
        ran_at: Timestamp::parse(&r.ran_at).unwrap_or_else(Timestamp::zero),
        why: r.why.clone(),
    }
}
