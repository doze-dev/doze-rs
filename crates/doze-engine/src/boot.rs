//! One start of one instance (Go's `bootLocked`), as a task: its needs woken and held first, then `setup`, the plan, the
//! processes gated on readiness, supervision, `on_ready`. Each step reports to the [`crate::machine::Machine`], which
//! decides what it means (a start cancelled by a stop or the shutdown is stopped as soon as it registers).

use crate::engine::{Engine, Phase};
use crate::error::Error;
use crate::events::{Cause, Event, Kind};
use crate::lifecycle::Now;
use crate::logs::RunLog;
use crate::machine::Unit;
use crate::process::idle_policy;
use crate::stack::{Endpoint, Kind as SvcKind, PlanCtx, Planned, Service, When};
use crate::steps::{self, Moment, StepEnv};
use crate::supervise::Handle;
use std::collections::BTreeMap;
use std::time::Instant;
use tokio::sync::watch;

/// Spawns start `boot` of `name`; `tx` hears how it ended.
/// A start the machine decided: its id, whether it only reruns `on_ready`, who hears how it ended, what cancels it.
#[derive(Debug)]
pub(crate) struct Start {
    pub(crate) boot: u64,
    pub(crate) rerun: bool,
    pub(crate) tx: watch::Sender<Phase>,
    pub(crate) cancel: watch::Receiver<bool>,
}

pub(crate) fn spawn(e: &Engine, name: &str, when: When, s: Start) {
    let Start {
        boot,
        rerun,
        tx,
        mut cancel,
    } = s;
    let (e, name) = (e.clone(), name.to_string());
    tokio::spawn(async move {
        let run = async {
            if rerun {
                rerun_on_ready(&e, &name, boot).await
            } else {
                start(&e, &name, boot, when).await
            }
        };
        // A cancel drops the start where it is: a process it spawned is a Tree (dropped: killed), a step's record goes
        // with its guard. Then the machine hears it, and the instance is asleep.
        let res = tokio::select! {
            biased;
            _ = cancel.wait_for(|c| *c) => {
                let fx = e.update(|c| c.m.boot_cancelled(&name, boot, Now::current()));
                e.run(fx);
                Err(Error::Failed(format!("{name} was stopped while it was starting")))
            }
            r = run => r,
        };
        e.update(|c| {
            c.boots.remove(&boot);
            c.cancels.remove(&boot);
        });
        let _ = tx.send(match res {
            Ok(ep) => Phase::Ready(ep),
            Err(err) => Phase::Failed(err),
        });
    });
}

async fn start(e: &Engine, name: &str, boot: u64, when: When) -> Result<Endpoint, Error> {
    let svc = e.service(name)?.clone();
    tracing::info!("waking \"{name}\"…");
    let started = Instant::now();
    let fail = |err: Error| {
        let fx = e.update(|c| {
            c.m.boot_failed(
                name,
                boot,
                &err.to_string(),
                started.elapsed(),
                Now::current(),
            )
        });
        e.run(fx);
        err
    };
    let needs = wake_needs(e, &svc, boot).await.map_err(fail)?;
    let ctx = PlanCtx {
        name: name.into(),
        data_dir: e.stack().cluster_dir(name),
        run_dir: e.stack().run_dir().join(name),
        needs,
    };
    let (planned, log) = prepare(e, &svc, &ctx).await.map_err(fail)?;
    let pids = e.pids(name);
    let unit =
        match crate::exec::start(&planned.plan, &log, planned.ready_within, Some(&pids)).await {
            Ok(u) => u,
            Err(err) => {
                pids.remove();
                log.close().await;
                let kind = if err.timeout {
                    Error::Timeout
                } else {
                    Error::Failed
                };
                return Err(fail(kind(err.message)));
            }
        };
    register(
        e,
        &svc,
        boot,
        &planned,
        Handle::supervise(unit),
        log.clone(),
    )?;
    if when == When::Started {
        return finish(e, name, boot, started, planned.endpoint);
    }
    on_ready(e, name, boot, &log, started).await?;
    finish(e, name, boot, started, planned.endpoint)
}

fn finish(
    e: &Engine,
    name: &str,
    boot: u64,
    started: Instant,
    ep: Endpoint,
) -> Result<Endpoint, Error> {
    let (ok, fx) = e.update(|c| c.m.boot_done(name, boot, started.elapsed(), Now::current()));
    e.run(fx);
    if ok {
        tracing::info!("\"{name}\" ready");
        Ok(ep)
    } else {
        Err(Error::Failed(format!(
            "{name} exited while it was getting ready"
        )))
    }
}

/// Wakes and holds every need, in order; a `Lazy` need is wired (its declared address), not woken.
async fn wake_needs(
    e: &Engine,
    svc: &Service,
    boot: u64,
) -> Result<BTreeMap<String, Endpoint>, Error> {
    let mut eps = BTreeMap::new();
    for need in svc.needs.iter().filter(|n| n.name != svc.name) {
        if need.when == When::Lazy {
            eps.insert(need.name.clone(), declared_endpoint(e, &need.name));
            continue;
        }
        let cause = Cause::Dependency(svc.name.clone());
        match Box::pin(e.wake_when(&need.name, cause, need.when)).await {
            Ok(ep) => {
                e.update(|c| c.m.need_held(&svc.name, boot, &need.name));
                eps.insert(need.name.clone(), ep);
            }
            Err(err) => {
                return Err(Error::Failed(format!(
                    "booting dependency \"{}\": {err}",
                    need.name
                )));
            }
        }
    }
    Ok(eps)
}

/// Where a need is reached before it ever ran: its last endpoint, else what its declaration says (a command's own port).
fn declared_endpoint(e: &Engine, name: &str) -> Endpoint {
    let last = e.update(|c| c.live.get(name).map(|l| l.endpoint.clone()));
    if let Some(ep) = last.filter(|ep| !ep.backend.is_empty()) {
        return ep;
    }
    match e.service(name).map(|s| &s.kind) {
        Ok(SvcKind::Process(p)) => Endpoint {
            backend: p
                .port
                .map(|p| format!("{}:{p}", crate::process::PRIVATE_HOST))
                .unwrap_or_default(),
        },
        Ok(SvcKind::Planned(p)) => {
            let ctx = PlanCtx {
                name: name.into(),
                data_dir: e.stack().cluster_dir(name),
                run_dir: e.stack().run_dir().join(name),
                needs: BTreeMap::new(),
            };
            p.endpoint(&ctx).unwrap_or_default()
        }
        Err(_) => Endpoint::default(),
    }
}

/// The folders, the log, `setup` when this data folder needs it (before the plan's checks, as Go), then the plan.
async fn prepare(e: &Engine, svc: &Service, ctx: &PlanCtx) -> Result<(Planned, RunLog), Error> {
    let io = |what: &str, err: std::io::Error| {
        Error::Failed(format!("{what} for \"{}\": {err}", svc.name))
    };
    doze_os::fs::private_dir(&ctx.data_dir).map_err(|err| io("creating the data folder", err))?;
    let log = RunLog::begin(e.logs(), &svc.name).map_err(|err| io("opening the log", err))?;
    match plan_for(e, svc, ctx, &log).await {
        Ok(planned) => Ok((planned, log)),
        Err(err) => {
            log.close().await;
            Err(err)
        }
    }
}

/// `setup` when this data folder needs it, then the plan, with doze's own variables in every process.
async fn plan_for(
    e: &Engine,
    svc: &Service,
    ctx: &PlanCtx,
    log: &RunLog,
) -> Result<Planned, Error> {
    let io = |what: &str, err: std::io::Error| {
        Error::Failed(format!("{what} for \"{}\": {err}", svc.name))
    };
    let planned = match &svc.kind {
        SvcKind::Process(spec) => {
            let private = if spec.proxied() {
                Some(free_port().map_err(|err| io("a free port", err))?)
            } else {
                None
            };
            let at = StepEnv {
                dir: spec.dir.clone(),
                env: {
                    let mut env = spec.env_for(ctx, private);
                    crate::process::with_extra(&mut env, &e.stack().extra_env);
                    env
                },
                pids: Some(e.pids(&svc.name)),
            };
            if steps::setup_needed(&ctx.data_dir, &spec.setup) {
                let ran = steps::run(Moment::Setup, &spec.setup, &at, log, |r| {
                    step_event(e, &svc.name, r)
                })
                .await;
                if let Err(msg) = ran {
                    e.update(|c| c.m.table_mut().set_tainted(&svc.name, true));
                    return Err(Error::Failed(format!("setup for \"{}\": {msg}", svc.name)));
                }
                if let Err(err) = steps::mark_setup_done(&ctx.data_dir, &spec.setup) {
                    tracing::warn!("could not record setup for \"{}\": {err}", svc.name);
                }
            }
            let planned = spec.planned(ctx, private).map_err(Error::Failed)?;
            let steps = (spec.on_ready.clone(), spec.stop.clone(), at);
            e.update(|c| c.live.entry(svc.name.clone()).or_default().steps = Some(steps));
            planned
        }
        SvcKind::Planned(p) => {
            // Bounded like a step: a planner that never answers must not hold the start (and every waiter) forever.
            let limit = crate::steps::STEP_LIMIT;
            match tokio::time::timeout(limit, p.plan(ctx)).await {
                Ok(planned) => planned.map_err(Error::Failed)?,
                Err(_) => {
                    return Err(Error::Failed(format!(
                        "planning \"{}\" did not finish within {}",
                        svc.name,
                        crate::godur::go_duration(limit)
                    )));
                }
            }
        }
    };
    let mut planned = planned;
    for p in &mut planned.plan.processes {
        crate::process::with_extra(&mut p.env, &e.stack().extra_env);
    }
    Ok(planned)
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

/// The unit runs: the machine records it (or, if the start was cancelled meanwhile, says to stop it), then it is
/// watched.
fn register(
    e: &Engine,
    svc: &Service,
    boot: u64,
    p: &Planned,
    handle: Handle,
    log: RunLog,
) -> Result<(), Error> {
    let proxied = matches!(&svc.kind, SvcKind::Process(s) if s.proxied());
    let (always_on, idle_after) = idle_policy(p.supervised, proxied, svc.sleep);
    let unit = Unit {
        pids: handle.pids().to_vec(),
        backend: p.endpoint.backend.clone(),
        restart: p.restart,
        supervised: p.supervised,
        always_on,
        idle_after,
    };
    let (generation, ok, fx) = e.update(|c| {
        let r = c.m.register(&svc.name, boot, unit, Now::current());
        c.handles.insert(r.0, handle.clone());
        let live = c.live.entry(svc.name.clone()).or_default();
        live.endpoint = p.endpoint.clone();
        live.log = Some(log);
        live.supervised = p.supervised;
        live.health = p.plan.processes.first().and_then(|q| q.ready.clone());
        if let Some(old) = live.watcher.take() {
            old.abort();
        }
        live.watcher = crate::watch::restarter(e, &svc.name, &svc.watch);
        r
    });
    tokio::spawn(crate::stop::watch(
        e.clone(),
        svc.name.clone(),
        generation,
        handle,
    ));
    e.run(fx);
    if ok {
        Ok(())
    } else {
        Err(Error::Failed(format!(
            "{} was stopped while it was starting",
            svc.name
        )))
    }
}

/// `on_ready`: when it fails the command keeps running (the step is what is broken), but the instance is not ready.
async fn on_ready(
    e: &Engine,
    name: &str,
    boot: u64,
    log: &RunLog,
    started: Instant,
) -> Result<(), Error> {
    let steps = e.update(|c| {
        c.live
            .get(name)
            .and_then(|l| l.steps.as_ref())
            .map(|(r, _, at)| (r.clone(), at.clone()))
    });
    let Some((steps, at)) = steps.filter(|(s, _)| !s.is_empty()) else {
        return Ok(());
    };
    let ran = steps::run(Moment::OnReady, &steps, &at, log, |r| {
        step_event(e, name, r)
    })
    .await;
    let Err(msg) = ran else {
        return Ok(());
    };
    let fx = e.update(|c| c.m.on_ready_failed(name, boot, &msg, started.elapsed()));
    e.run(fx);
    Err(Error::Failed(format!("on_ready for \"{name}\": {msg}")))
}

/// Running but left not ready by a failed `on_ready`: run that again, and nothing else.
async fn rerun_on_ready(e: &Engine, name: &str, boot: u64) -> Result<Endpoint, Error> {
    let started = Instant::now();
    let log = e.update(|c| c.live.get(name).and_then(|l| l.log.clone()));
    let Some(log) = log else {
        return Err(Error::Failed(format!(
            "{name} has no log to run on_ready against"
        )));
    };
    on_ready(e, name, boot, &log, started).await?;
    let ep = e.update(|c| {
        c.live
            .get(name)
            .map(|l| l.endpoint.clone())
            .unwrap_or_default()
    });
    finish(e, name, boot, started, ep)
}
