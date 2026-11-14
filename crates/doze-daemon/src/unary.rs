//! The unary ops (Go's `server.go` dispatch and `daemon/handler.go`): each answers with one [`Response`]. They run on
//! the daemon's runtime and to their end: a client that goes away does not abort a wake (Go ran them on the daemon's
//! context).

use crate::Daemon;
use crate::view::{self, go_duration};
use doze_control::{Op, Response, Timestamp, fault};
use doze_engine::{Cause, Error};

/// What `logs` reads when the request does not say (Go's `logBacklog`).
const LOG_BACKLOG: usize = 200;

pub(crate) fn answer(d: &Daemon, req: &doze_control::Request) -> Response {
    let Some(op) = req.known_op() else {
        return failed(&format!("unknown op: {}", req.op), "");
    };
    let rt = d.inner.rt.clone();
    let done = match op {
        Op::Status => return status(d),
        Op::Logs => return logs(d, req),
        Op::Up => rt.block_on(up(d, &req.db)),
        Op::Boot | Op::Wake => rt.block_on(wake(d, &req.db)),
        Op::Down | Op::Sleep => rt.block_on(down(d, &req.db)),
        Op::Restart => rt.block_on(restart(d, &req.db)),
        Op::Task => rt.block_on(d.engine().run_task(&req.db)),
        Op::KeepAwake | Op::Pin => pin(d, &req.db),
        Op::Off | Op::On => rt.block_on(off(d, &req.db, op == Op::Off)),
        Op::Apply => rt.block_on(d.engine().sync(target(&req.db))).map(|_| ()),
        Op::Destroy => rt.block_on(d.engine().destroy(target(&req.db))),
        Op::Reset | Op::Add | Op::Remove | Op::Resources | Op::Admin => Err(Error::Failed(
            format!("{} is not in the Rust daemon yet", op.as_str()),
        )),
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

fn status(d: &Daemon) -> Response {
    let e = d.engine();
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
        instances: view::snapshot(e, &d.inner.labels),
        ..Response::default()
    }
}

fn paused(d: &Daemon, name: &str) -> bool {
    d.inner.labels.get(name).is_some_and(|l| l.paused)
}

async fn wake(d: &Daemon, name: &str) -> Result<(), Error> {
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
fn logs(d: &Daemon, req: &doze_control::Request) -> Response {
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
        .unwrap_or(LOG_BACKLOG);
    match e.logs().tail(name, req.run, n) {
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
