//! Unary answers, as Go's `Server.unary` dispatches them to a fakedaemon: the scripted status, verbs that change it as
//! the daemon would (and tell subscribers), log reads with run, tail and since, and data actions.

use super::State;
use super::script::Fail;
use crate::client::connection::lock;
use crate::{Frame, InstanceView, LogFrame, Op, Request, Response, Topic};

/// The answer to one unary request.
pub(crate) fn answer(state: &State, req: &Request) -> Response {
    let Some(op) = req.known_op() else {
        return Response {
            error: format!("unknown op: {}", req.op),
            ..Response::default()
        };
    };
    match op {
        Op::Status => Response {
            ok: true,
            ..lock(&state.script).status.clone()
        },
        Op::Logs => logs(state, req),
        Op::Admin => {
            pause(state);
            let s = lock(&state.script);
            outcome(
                s.failure("admin", &req.action),
                Response {
                    result: s.admin_result.clone(),
                    ..Response::default()
                },
            )
        }
        _ => {
            pause(state);
            let fail = lock(&state.script).failure(&req.op, &req.db).cloned();
            if fail.is_none() {
                change(state, op, &req.db);
            }
            outcome(fail.as_ref(), Response::default())
        }
    }
}

/// Waits as long as the script says a verb takes.
fn pause(state: &State) {
    let d = lock(&state.script).verb_delay;
    if !d.is_zero() {
        std::thread::sleep(d);
    }
}

fn outcome(fail: Option<&Fail>, ok: Response) -> Response {
    match fail {
        Some(f) => Response {
            error: f.error.clone(),
            code: f.code.clone(),
            ..Response::default()
        },
        None => Response { ok: true, ..ok },
    }
}

/// Changes the status the way a verb would, and tells held subscribers with a snapshot.
fn change(state: &State, op: Op, target: &str) {
    let instances = {
        let mut s = lock(&state.script);
        let mine = s.status.instances.iter_mut().filter(|i| i.name == target);
        let apply: fn(&mut InstanceView) = match op {
            Op::Pin | Op::KeepAwake => |i| i.keep_awake = !i.keep_awake,
            Op::Sleep | Op::Down => |i| {
                if !i.disabled {
                    (i.state, i.ram, i.cpu, i.conns) = ("reaped".into(), 0, 0.0, 0);
                }
            },
            Op::Wake | Op::Boot => |i| {
                if !i.disabled {
                    i.state = "active".into();
                }
            },
            Op::Off => |i| {
                i.disabled = true;
                (i.state, i.ram, i.cpu, i.conns) = ("disabled".into(), 0, 0.0, 0);
            },
            Op::On => |i| {
                i.disabled = false;
                i.state = "reaped".into();
            },
            _ => return,
        };
        mine.for_each(apply);
        s.status.instances.clone()
    };
    state.push(&Frame {
        topic: Topic::Instances.as_str().into(),
        snapshot: true,
        instances,
        ..Frame::default()
    });
}

/// The lines an instance printed: run 0 is every line, another run only its own; then since, then tail.
fn logs(state: &State, req: &Request) -> Response {
    let s = lock(&state.script);
    let declared = s.status.instances.iter().any(|i| i.name == req.db)
        || s.logs.iter().any(|l| l.instance == req.db);
    if !declared {
        return Response {
            error: format!("instance {:?} is not declared", req.db),
            ..Response::default()
        };
    }
    let since = req
        .since_time
        .unix_nanos()
        .filter(|_| !req.since_time.is_zero());
    let mut log: Vec<LogFrame> = s
        .logs
        .iter()
        .filter(|l| l.instance == req.db)
        .filter(|l| req.run <= 0 || l.run == req.run)
        .filter(|l| since.is_none_or(|t| l.time.unix_nanos().is_some_and(|lt| lt >= t)))
        .cloned()
        .collect();
    if let Ok(tail) = usize::try_from(req.tail)
        && tail > 0
        && log.len() > tail
    {
        log.drain(..log.len() - tail);
    }
    let lines = log.iter().map(|l| l.line.clone()).collect();
    Response {
        ok: true,
        lines,
        log,
        ..Response::default()
    }
}
