//! The streams (Go's `server.go` stream path and `daemon/subscribe.go`): `subscribe` with its topics, `events`, and
//! `logs` with `follow`. A stream is acknowledged with `{ok: true}`, sends its frames, and ends with `end` (and the
//! error, if it failed and was not cancelled).

use crate::session::{Writer, reply, send};
use crate::{Daemon, view};
use doze_control::{
    Frame, InstanceView, JOURNAL_FROM_START, LogFrame, Op, Response, StatView, Timestamp,
};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::watch;

/// The least time between two instances frames (Go's `instancesMinGap`).
const INSTANCES_GAP: Duration = Duration::from_millis(100);
/// Stats every second unless asked otherwise, never more often than every 250 ms (Go's).
const STATS_GAP: Duration = Duration::from_secs(1);
const STATS_MIN_GAP: Duration = Duration::from_millis(250);
/// What the logs topic sends of each instance before following (Go's `logBacklog`).
const LOG_BACKLOG: i64 = 200;

/// A topic's outcome: `Err` ends the stream with that error.
type Done = Result<(), String>;

pub(crate) fn run(d: &Daemon, w: &Writer, req: &doze_control::Request, end: watch::Receiver<bool>) {
    let op = req.known_op();
    let interval = if req.interval.is_empty() {
        Ok(STATS_GAP)
    } else {
        doze_control::parse_go_duration(&req.interval).ok_or(())
    };
    // Go's checks before the acknowledgement, answered as a unary error.
    if op == Some(Op::Subscribe) {
        let refused = match &interval {
            Err(()) => Some(format!("subscribe: {:?} is not an interval", req.interval)),
            Ok(_) if req.topics.is_empty() => Some("subscribe: name at least one topic".into()),
            Ok(_) => None,
        };
        if let Some(error) = refused {
            let r = Response {
                error,
                ..Response::default()
            };
            let _ = send(w, &reply(req.id, r));
            return;
        }
    }
    let ok = Response {
        ok: true,
        ..Response::default()
    };
    if send(w, &reply(req.id, ok)).is_err() {
        return;
    }
    let gap = interval.unwrap_or(STATS_GAP).max(STATS_MIN_GAP);
    let mut ended = end.clone();
    let body = async {
        match op {
            Some(Op::Subscribe) => subscribe(d, w, req, gap).await,
            Some(Op::Events) => events(d, w, req.id).await,
            _ => logs_for(d, w, req.id, &req.names, -1).await,
        }
    };
    let result = d.inner.rt.block_on(async {
        tokio::select! {
            _ = ended.wait_for(|e| *e) => Ok(()),
            r = body => r,
        }
    });
    let error = match result {
        Err(e) if !*end.borrow() => e,
        _ => String::new(),
    };
    let last = Frame {
        id: req.id,
        end: true,
        error,
        ..Frame::default()
    };
    let _ = send(w, &last);
}

async fn subscribe(d: &Daemon, w: &Writer, req: &doze_control::Request, gap: Duration) -> Done {
    let mut set = tokio::task::JoinSet::new();
    for t in &req.topics {
        let (d, w, names) = (d.clone(), w.clone(), req.names.clone());
        let (since, backlog, id) = (req.since, req.backlog, req.id);
        match t.as_str() {
            "instances" => set.spawn(async move { instances(&d, &w, id).await }),
            "stats" => set.spawn(async move { stats(&d, &w, id, gap).await }),
            "journal" => set.spawn(async move { journal(&d, &w, id, since).await }),
            "logs" => set.spawn(async move { logs_for(&d, &w, id, &names, backlog).await }),
            "console" => set.spawn(async {
                Err("subscribe: the console topic is not in the Rust daemon yet".into())
            }),
            other => {
                return Err(format!(
                    "subscribe: no topic called {other:?} (want instances, stats, journal, logs or console)"
                ));
            }
        };
    }
    while let Some(r) = set.join_next().await {
        r.map_err(|e| e.to_string())??;
    }
    Ok(())
}

fn emit(w: &Writer, id: u64, mut f: Frame) -> Done {
    f.id = id;
    send(w, &f).map_err(|e| e.to_string())
}

/// A full snapshot first, then the instances that changed (and the ones gone), at most every 100 ms.
async fn instances(d: &Daemon, w: &Writer, id: u64) -> Done {
    let mut changes = d.engine().changes();
    let mut last: BTreeMap<String, InstanceView> = BTreeMap::new();
    let mut first = true;
    loop {
        let now = view::snapshot(d.engine(), &d.inner.labels);
        let mut frame = Frame {
            topic: "instances".into(),
            snapshot: first,
            ..Frame::default()
        };
        let present: Vec<String> = now.iter().map(|v| v.name.clone()).collect();
        for v in now {
            if first || last.get(&v.name) != Some(&v) {
                last.insert(v.name.clone(), v.clone());
                frame.instances.push(v);
            }
        }
        last.retain(|n, _| {
            let keep = present.contains(n);
            if !keep {
                frame.removed.push(n.clone());
            }
            keep
        });
        if first || !frame.instances.is_empty() || !frame.removed.is_empty() {
            emit(w, id, frame)?;
        }
        first = false;
        changes.changed().await.map_err(|e| e.to_string())?;
        tokio::time::sleep(INSTANCES_GAP).await;
    }
}

/// Each running instance's pid and connections every `gap`. Its RAM and CPU are not sampled yet (D1 open item).
async fn stats(d: &Daemon, w: &Writer, id: u64, gap: Duration) -> Done {
    loop {
        let stats = d
            .engine()
            .snapshot()
            .into_iter()
            .filter(|i| i.state.running())
            .map(|i| StatView {
                name: i.name,
                pid: i64::from(i.pid),
                conns: i64::from(i.conns),
                ..StatView::default()
            })
            .collect();
        let frame = Frame {
            topic: "stats".into(),
            at: Timestamp::from_system_time(std::time::SystemTime::now()),
            stats,
            ..Frame::default()
        };
        emit(w, id, frame)?;
        tokio::time::sleep(gap).await;
    }
}

fn wire_event(e: &doze_journal::Event) -> Option<doze_control::Event> {
    serde_json::from_str(&doze_journal::encode(e)).ok()
}

fn journal_frame(e: &doze_journal::Event) -> Option<Frame> {
    Some(Frame {
        topic: "journal".into(),
        event: Some(wire_event(e)?),
        ..Frame::default()
    })
}

/// Go's `subJournal`: what was kept after `since` (everything for [`JOURNAL_FROM_START`], nothing for 0), then each
/// event as it is appended, with a gap the subscriber fell behind on filled from disk.
async fn journal(d: &Daemon, w: &Writer, id: u64, since: u64) -> Done {
    let Some(j) = d.engine().journal() else {
        return Ok(());
    };
    let mut live = j.subscribe();
    let mut last = 0;
    if since != 0 {
        let from = if since == JOURNAL_FROM_START {
            0
        } else {
            since
        };
        for e in j.since(from).map_err(|e| e.to_string())? {
            if let Some(f) = journal_frame(&e) {
                emit(w, id, f)?;
            }
            last = e.seq;
        }
    }
    loop {
        let e = match live.recv().await {
            Ok(e) => e,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(err) => return Err(err.to_string()),
        };
        if e.seq <= last {
            continue;
        }
        if last != 0 && e.seq > last + 1 {
            for m in j.since(last).map_err(|e| e.to_string())? {
                if m.seq >= e.seq {
                    break;
                }
                if let Some(f) = journal_frame(&m) {
                    emit(w, id, f)?;
                }
            }
        }
        if let Some(f) = journal_frame(&e) {
            emit(w, id, f)?;
        }
        last = e.seq;
    }
}

/// `events`: each instance as it changes, one per frame (Go's `StreamEvents`).
async fn events(d: &Daemon, w: &Writer, id: u64) -> Done {
    let mut moved = d.engine().transitions();
    loop {
        let inst = match moved.recv().await {
            Ok(i) => i,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(err) => return Err(err.to_string()),
        };
        let v = view::view(d.engine(), &inst, d.inner.labels.get(&inst.name));
        let frame = Frame {
            topic: "instances".into(),
            instances: vec![v],
            ..Frame::default()
        };
        emit(w, id, frame)?;
    }
}

pub(crate) fn log_frame(l: &doze_logstore::Line) -> LogFrame {
    LogFrame {
        instance: l.instance.clone(),
        line: l.text.clone(),
        stream: l.stream.clone(),
        run: i64::from(l.run),
        time: Timestamp::parse(&l.stamp).unwrap_or_else(Timestamp::zero),
    }
}

/// The logs topic: `backlog` recent lines of each name (every declared instance when none), then follows them.
async fn logs_for(d: &Daemon, w: &Writer, id: u64, names: &[String], backlog: i64) -> Done {
    let names: Vec<String> = if names.is_empty() {
        d.engine()
            .stack()
            .services
            .iter()
            .map(|s| s.name.clone())
            .collect()
    } else {
        names.to_vec()
    };
    let backlog = if backlog == 0 { LOG_BACKLOG } else { backlog };
    let mut set = tokio::task::JoinSet::new();
    for n in names {
        let (d, w) = (d.clone(), w.clone());
        set.spawn(async move { follow_one(&d, &w, id, &n, backlog).await });
    }
    while let Some(r) = set.join_next().await {
        r.map_err(|e| e.to_string())??;
    }
    Ok(())
}

async fn follow_one(d: &Daemon, w: &Writer, id: u64, name: &str, backlog: i64) -> Done {
    let store = d.engine().logs();
    let n = usize::try_from(backlog).unwrap_or(0);
    let (lines, from) = store.tail_at(name, 0, n).map_err(|e| e.to_string())?;
    for l in &lines {
        emit(w, id, log_line(l))?;
    }
    let mut failed = None;
    store
        .follow(name, from, |l| match emit(w, id, log_line(&l)) {
            Ok(()) => true,
            Err(e) => {
                failed = Some(e);
                false
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    failed.map_or(Ok(()), Err)
}

fn log_line(l: &doze_logstore::Line) -> Frame {
    Frame {
        topic: "logs".into(),
        log: Some(log_frame(l)),
        ..Frame::default()
    }
}
