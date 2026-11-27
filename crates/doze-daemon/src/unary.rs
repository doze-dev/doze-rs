//! The unary ops (Go's `server.go` dispatch and `daemon/handler.go`): each answers with one [`Response`]. Each runs
//! as a task on the daemon's runtime, and to its end: a client that goes away does not abort a wake (Go ran them on
//! the daemon's context). Nothing here may block a worker (there are two, and they supervise every service): disk work
//! goes to the blocking threads.

use crate::Daemon;
use crate::view::{self, go_duration};
use doze_control::{Op, Response, Timestamp, fault};
use doze_engine::{Cause, Error};

/// What `logs` reads when the request does not say (Go's `logBacklog`).
const LOG_BACKLOG: usize = 200;
/// The most lines one `logs` answer carries, whatever it asks for: the app asks for the default, the CLI for what
/// `--tail` says. The store keeps 10 MiB per instance, so a run rarely has more; one answer holds them all in memory.
const LOG_MOST: usize = 10_000;

pub(crate) async fn answer(d: &Daemon, req: &doze_control::Request) -> Response {
    let Some(op) = req.known_op() else {
        return failed(&format!("unknown op: {}", req.op), "");
    };
    let done = match op {
        Op::Status => return status(d).await,
        Op::Logs => return logs(d, req).await,
        Op::Up => up(d, &req.db).await,
        Op::Boot | Op::Wake => wake(d, &req.db).await,
        Op::Down | Op::Sleep => down(d, &req.db).await,
        Op::Restart => restart(d, &req.db).await,
        Op::Task => d.engine().run_task(&req.db).await,
        Op::KeepAwake | Op::Pin => pin(d, &req.db),
        Op::Off | Op::On => off(d, &req.db, op == Op::Off).await,
        Op::Apply => d.engine().sync(target(&req.db)).await.map(|_| ()),
        Op::Destroy => d.engine().destroy(target(&req.db)).await,
        Op::Reset => return crate::admin::reset(d, &req.db).await,
        Op::Resources => return crate::admin::resources(d, &req.db).await,
        Op::Admin => return crate::admin::admin(d, req).await,
        Op::Plan => return plan(d, &req.db).await,
        Op::Migrate => return crate::admin::migrate(d, &req.db).await,
        Op::Lint => return crate::lint::lint(d, &req.input).await,
        Op::Value => return crate::blocks::value(d, req).await,
        Op::Children => return crate::blocks::children(d, req).await,
        Op::Add => return crate::live::add(d, &req.block).await,
        Op::Remove => return crate::live::remove(d, &req.db, req.wipe).await,
        // Streams and cancel are the session's.
        Op::Subscribe | Op::Events | Op::Cancel => {
            return failed(&format!("unknown op: {}", req.op), "");
        }
    };
    match done {
        Ok(()) => Response {
            ok: true,
            ..Response::default()
        },
        Err(err) => failed(&err.to_string(), code(&err)),
    }
}

fn target(db: &str) -> Option<&str> {
    (!db.is_empty()).then_some(db)
}

/// Go's `fault` kind for an engine error; `failed` is left out on the wire.
fn code(err: &Error) -> &'static str {
    match err {
        Error::NotDeclared { .. } => fault::NOT_FOUND,
        Error::Timeout(_) => fault::TIMEOUT,
        _ => "",
    }
}

fn failed(error: &str, code: &str) -> Response {
    Response {
        ok: false,
        error: error.into(),
        code: code.into(),
        ..Response::default()
    }
}

async fn status(d: &Daemon) -> Response {
    let e = d.engine();
    let mut views = view::snapshot(e, &d.labels());
    crate::meters::fill(d, &mut views).await;
    // A `stat` per client: on a blocking thread.
    let owned = d.clone();
    let views = tokio::task::spawn_blocking(move || {
        crate::client::fill_paths(&owned, &mut views);
        views
    })
    .await
    .unwrap_or_default();
    Response {
        ok: true,
        listen: d.inner.info.listen.clone(),
        idle_timeout: e
            .stack()
            .idle_timeout
            .map_or(doze_control::GoDuration(0), go_duration),
        stack: e.stack().name.clone(),
        dir: d.inner.info.dir.clone(),
        loaded: d
            .inner
            .info
            .loaded
            .map_or_else(Timestamp::zero, Timestamp::from_system_time),
        instances: views,
        ..Response::default()
    }
}

/// What `apply` would change (the `plan` op).
async fn plan(d: &Daemon, db: &str) -> Response {
    match d.engine().plan(target(db)).await {
        Ok(planned) => Response {
            ok: true,
            changes: planned
                .into_iter()
                .flat_map(|(instance, changes)| {
                    changes
                        .into_iter()
                        .map(move |(c, o)| doze_control::ChangeView {
                            instance: instance.clone(),
                            change: c.as_str().into(),
                            kind: o.kind,
                            name: o.name,
                        })
                })
                .collect(),
            ..Response::default()
        },
        Err(err) => failed(&err.to_string(), code(&err)),
    }
}

fn paused(d: &Daemon, name: &str) -> bool {
    d.label(name).is_some_and(|l| l.paused)
}

pub(crate) async fn wake(d: &Daemon, name: &str) -> Result<(), Error> {
    if paused(d, name) {
        return Err(Error::Failed(format!(
            "instance \"{name}\" is disabled (enabled = false); enable it in the config to wake it"
        )));
    }
    d.engine().wake(name, Cause::Command).await.map(|_| ())
}

/// Go's `Up`: one instance, or every one the config has not paused, in the config's order.
async fn up(d: &Daemon, name: &str) -> Result<(), Error> {
    if !name.is_empty() {
        return d.engine().wake(name, Cause::Command).await.map(|_| ());
    }
    let names: Vec<String> = d
        .engine()
        .stack()
        .services
        .iter()
        .map(|s| s.name.clone())
        .filter(|n| !paused(d, n))
        .collect();
    for n in names {
        d.engine().wake(&n, Cause::Command).await?;
    }
    Ok(())
}

async fn down(d: &Daemon, name: &str) -> Result<(), Error> {
    if name.is_empty() {
        d.engine().stop_all(Cause::Command).await;
        return Ok(());
    }
    d.engine().stop(name, Cause::Command).await
}

async fn restart(d: &Daemon, name: &str) -> Result<(), Error> {
    d.engine().stop(name, Cause::Command).await?;
    d.engine().wake(name, Cause::Command).await.map(|_| ())
}

fn pin(d: &Daemon, name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::Failed("keepawake needs an instance name".into()));
    }
    d.engine().toggle_pin(name).map(|_| ())
}

async fn off(d: &Daemon, name: &str, off: bool) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::Failed("off and on need an instance name".into()));
    }
    d.engine().set_off(name, off).await
}

/// Go's `LogLines`: the last lines of one run (the latest when `run` is 0), as frames and as plain lines.
async fn logs(d: &Daemon, req: &doze_control::Request) -> Response {
    let e = d.engine();
    let name = req.db.as_str();
    if e.stack().service(name).is_none() {
        let err = Error::NotDeclared {
            name: name.into(),
            config: e.stack().config_name.clone(),
        };
        return failed(&err.to_string(), fault::NOT_FOUND);
    }
    let n = usize::try_from(req.tail)
        .ok()
        .filter(|n| *n > 0)
        .unwrap_or(LOG_BACKLOG)
        .min(LOG_MOST);
    // Read from disk: on a blocking thread.
    let (store, inst, run) = (e.logs().clone(), name.to_owned(), req.run);
    let tail = match tokio::task::spawn_blocking(move || store.tail(&inst, run, n)).await {
        Ok(t) => t,
        Err(err) => return failed(&err.to_string(), ""),
    };
    match tail {
        Ok(lines) => {
            let log: Vec<_> = lines.iter().map(crate::topics::log_frame).collect();
            Response {
                ok: true,
                lines: log.iter().map(|l| l.line.clone()).collect(),
                log,
                ..Response::default()
            }
        }
        Err(err) => failed(&err.to_string(), ""),
    }
}
