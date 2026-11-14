//! One start of one instance (Go's `bootLocked`): its needs woken and held first, then its plan, `setup`, the processes
//! gated on readiness, supervision, `on_ready`. A failure before the processes run releases the needs and marks it
//! failed; a failed `on_ready` leaves the processes running but the instance not ready.

use crate::engine::Engine;
use crate::error::Error;
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::Now;
use crate::logs::RunLog;
use crate::process::idle_policy;
use crate::stack::{Endpoint, Kind as SvcKind, PlanCtx, Planned, Service, When};
use crate::steps::{self, Moment, StepEnv};
use crate::supervise::Handle;
use std::collections::BTreeMap;
use std::time::Instant;

/// Starts `name` for `cause`, to `when`.
pub(crate) async fn boot(
    e: &Engine,
    name: &str,
    cause: Cause,
    when: When,
) -> Result<Endpoint, Error> {
    if let Some(r) = rerun_on_ready(e, name, cause.clone()).await {
        return r;
    }
    let svc = e.service(name)?.clone();
    e.update(|c| c.table.mark_booting(name));
    e.event(Event::new(Kind::Wake, name).cause(cause));
    tracing::info!("waking \"{name}\"…");
    let started = Instant::now();
    let fail = |held: &[String], err: Error| {
        for h in held {
            e.release(h);
        }
        e.event(
            Event::new(Kind::Failed, name)
                .took(started.elapsed())
                .detail(err.to_string()),
        );
        e.update(|c| c.table.mark_failed(name, &err.to_string()));
        err
    };
    let (held, needs) = match wake_needs(e, &svc).await {
        Ok(x) => x,
        Err(err) => return Err(fail(&[], err)),
    };
    let ctx = PlanCtx {
        name: name.into(),
        data_dir: e.stack().cluster_dir(name),
        run_dir: e.stack().run_dir().join(name),
        needs,
    };
    let (planned, log) = match prepare(e, &svc, &ctx).await {
        Ok(x) => x,
        Err(err) => return Err(fail(&held, err)),
    };
    let unit = match crate::exec::start(&planned.plan, &log, planned.ready_within).await {
        Ok(u) => u,
        Err(msg) => return Err(fail(&held, Error::Failed(msg))),
    };
    let handle = Handle::supervise(unit);
    register(e, &svc, &planned, held, handle, log.clone());
    if when != When::Started {
        on_ready(e, &svc, &log, started).await?;
    }
    e.event(Event::new(Kind::Ready, name).took(started.elapsed()));
    tracing::info!("\"{name}\" ready");
    Ok(planned.endpoint)
}

/// Wakes and holds every need (in order) before the start; a need woken `Lazy` is wired, not woken. On a failure the
/// needs already held are released.
async fn wake_needs(
    e: &Engine,
    svc: &Service,
) -> Result<(Vec<String>, BTreeMap<String, Endpoint>), Error> {
    let mut held: Vec<String> = Vec::new();
    let mut eps = BTreeMap::new();
    for need in &svc.needs {
        if need.name == svc.name {
            continue;
        }
        if need.when == When::Lazy {
            let ep = e.live_endpoint(&need.name);
            eps.insert(need.name.clone(), ep);
            continue;
        }
        let cause = Cause::Dependency(svc.name.clone());
        match Box::pin(e.wake_when(&need.name, cause, need.when)).await {
            Ok(ep) => {
                e.acquire(&need.name);
                held.push(need.name.clone());
                eps.insert(need.name.clone(), ep);
            }
            Err(err) => {
                for h in &held {
                    e.release(h);
                }
                return Err(Error::Failed(format!(
                    "booting dependency \"{}\": {err}",
                    need.name
                )));
            }
        }
    }
    Ok((held, eps))
}

/// The plan, the folders, the log, and `setup` when this data folder needs it.
async fn prepare(e: &Engine, svc: &Service, ctx: &PlanCtx) -> Result<(Planned, RunLog), Error> {
    let io = |what: &str, err: std::io::Error| {
        Error::Failed(format!("{what} for \"{}\": {err}", svc.name))
    };
    doze_os::fs::private_dir(&ctx.data_dir).map_err(|err| io("creating the data folder", err))?;
    let log =
        RunLog::open(&e.stack().run_dir(), &svc.name).map_err(|err| io("opening the log", err))?;
    let planned = match &svc.kind {
        SvcKind::Process(spec) => {
            let private = if spec.proxied() {
                Some(free_port().map_err(|err| io("a free port", err))?)
            } else {
                None
            };
            let planned = spec.planned(ctx, private).map_err(Error::Failed)?;
            if steps::setup_needed(&ctx.data_dir, &spec.setup) {
                let at = StepEnv {
                    dir: spec.dir.clone(),
                    env: spec.env_for(ctx, private),
                };
                let ran = steps::run(Moment::Setup, &spec.setup, &at, &log, |r| {
                    step_event(e, &svc.name, r)
                })
                .await;
                if let Err(msg) = ran {
                    e.update(|c| c.table.set_tainted(&svc.name, true));
                    return Err(Error::Failed(format!("setup for \"{}\": {msg}", svc.name)));
                }
                if let Err(err) = steps::mark_setup_done(&ctx.data_dir, &spec.setup) {
                    tracing::warn!("could not record setup for \"{}\": {err}", svc.name);
                }
            }
            e.remember_steps(&svc.name, spec, ctx, private);
            planned
        }
        SvcKind::Planned(p) => p.plan(ctx).await.map_err(Error::Failed)?,
    };
    Ok((planned, log))
}

/// A port nothing listens on now (Go's `netutil.FreePort`).
fn free_port() -> std::io::Result<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()
        .map(|a| a.port())
}

pub(crate) fn step_event(e: &Engine, name: &str, r: steps::Ran) {
    e.event(Event::new(Kind::Step, name).took(r.took).detail(r.detail));
}

/// The unit runs: record it, mark it running, write its pidfiles, watch it.
fn register(
    e: &Engine,
    svc: &Service,
    planned: &Planned,
    held: Vec<String>,
    handle: Handle,
    log: RunLog,
) {
    let proxied = matches!(&svc.kind, SvcKind::Process(p) if p.proxied());
    let (always_on, idle_after) = idle_policy(planned.supervised, proxied, svc.sleep);
    let generation = e.update(|c| {
        c.next_gen += 1;
        let generation = c.next_gen;
        let live = c.live.entry(svc.name.clone()).or_default();
        live.unit = Some((generation, handle.clone()));
        live.held = held;
        live.endpoint = planned.endpoint.clone();
        live.log = Some(log);
        live.ready_pending = false;
        live.unit_in_flight = true;
        live.health = planned.plan.processes.first().and_then(|p| p.ready.clone());
        c.table.set_idle_policy(&svc.name, always_on, idle_after);
        c.table.mark_running(
            &svc.name,
            &planned.endpoint.backend,
            handle.pid(),
            Now::current(),
        );
        generation
    });
    crate::pidfile::write(&e.stack().run_dir(), &svc.name, handle.pids());
    let watch = crate::stop::Watch {
        name: svc.name.clone(),
        generation,
        restart: planned.restart,
        supervised: planned.supervised,
    };
    tokio::spawn(crate::stop::watch(e.clone(), watch, handle));
}

/// `on_ready`: when it fails the command keeps running (the step is what is broken), but the instance is not ready.
async fn on_ready(e: &Engine, svc: &Service, log: &RunLog, started: Instant) -> Result<(), Error> {
    let Some((steps, at)) = e.on_ready_steps(&svc.name) else {
        return Ok(());
    };
    if steps.is_empty() {
        return Ok(());
    }
    let ran = steps::run(Moment::OnReady, &steps, &at, log, |r| {
        step_event(e, &svc.name, r)
    })
    .await;
    let Err(msg) = ran else {
        return Ok(());
    };
    let err = Error::Failed(format!("on_ready for \"{}\": {msg}", svc.name));
    e.update(|c| {
        if let Some(l) = c.live.get_mut(&svc.name) {
            l.ready_pending = true;
        }
        c.table.mark_failed(&svc.name, &msg);
    });
    e.event(
        Event::new(Kind::Failed, &svc.name)
            .took(started.elapsed())
            .detail(msg),
    );
    Err(err)
}

/// Running but left not ready by a failed `on_ready`: run that again, and nothing else.
async fn rerun_on_ready(e: &Engine, name: &str, cause: Cause) -> Option<Result<Endpoint, Error>> {
    let (log, pid, backend) = e.pending_ready(name)?;
    let started = Instant::now();
    e.event(
        Event::new(Kind::Wake, name)
            .cause(cause)
            .detail("on_ready again"),
    );
    let svc = e.service(name).ok()?.clone();
    if let Err(err) = on_ready(e, &svc, &log, started).await {
        return Some(Err(err));
    }
    let ep = e.update(|c| {
        c.table.mark_running(name, &backend, pid, Now::current());
        c.live.get_mut(name).map_or_else(Endpoint::default, |l| {
            l.ready_pending = false;
            l.endpoint.clone()
        })
    });
    e.event(Event::new(Kind::Ready, name).took(started.elapsed()));
    Some(Ok(ep))
}
