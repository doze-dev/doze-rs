//! Replays: the archived events of a window put back on the archive's bus, through its rules (optionally only those named).
//!
//! A replay runs over time, as AWS's does: `StartReplay` answers `STARTING` and puts nothing back; the timer moves it to `RUNNING` and
//! puts the events back a batch at a time (`replay_run.rs`), and it is `COMPLETED` once the window is read. Until then `CancelReplay`
//! stops it: `CANCELLING` at once, `CANCELLED` at its next step, and events not yet put back stay unsent. AWS takes minutes to start a
//! replay (memory); here it starts after [`STARTING_MS`](crate::replay_run::STARTING_MS), long enough to be cancelled and short enough
//! to wait for.
//!
//! The wording of the refusals is from memory.

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::{Replay, Run, RunState, key};
use crate::names::{Parts, replay_arn};
use crate::page::{page, with_token};
use crate::replay_run::STARTING_MS;
use crate::req::{Req, exists, invalid, not_found};
use crate::util::{Out, secs};

/// A replay as the describe and list operations show it, whichever table it is in.
struct Shown<'a> {
    p: &'a Replay,
    state: &'static str,
    reason: Option<&'a str>,
    /// Whether it has stopped (and so has an end time).
    ended: bool,
}

impl Core {
    pub(crate) fn start_replay(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ReplayName");
        let k = key(r.account(), &[name]);
        if self.replays.get(&k).is_some() || self.runs.get(&k).is_some() {
            return Err(exists(format!("Replay {name} already exists.")));
        }
        let archive_name = Parts::of(r.s("EventSourceArn"))
            .filter(|p| p.kind == "archive")
            .map(|p| p.tail)
            .ok_or_else(|| invalid("Parameter EventSourceArn is not valid."))?;
        let archive = self
            .archives
            .get(&key(r.account(), &[archive_name]))
            .ok_or_else(|| not_found(format!("Archive {archive_name} does not exist.")))?
            .clone();
        let (start, end) = (secs_of(r, "EventStartTime"), secs_of(r, "EventEndTime"));
        if end <= start {
            return Err(invalid(
                "Parameter EventEndTime must be after EventStartTime.",
            ));
        }
        let destination = r.v.get("Destination").cloned().unwrap_or(Value::Null);
        let target = destination
            .get("Arn")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if target != archive.source_arn {
            return Err(invalid(
                "Parameter Destination.Arn is not valid. Reason: Destination must be the event bus the archive is from.",
            ));
        }
        let only = filter_arns(&destination);
        let bus = Parts::of(target).map(|p| p.tail).unwrap_or_default();
        let arn = replay_arn(r.region(), r.account(), name);
        let now = self.now_ms();
        let replay = Replay {
            account: r.account().to_owned(),
            name: name.to_owned(),
            arn: arn.clone(),
            description: r.opt("Description").map(str::to_owned),
            source_arn: archive.arn,
            destination: destination.to_string(),
            start_ms: start,
            end_ms: end,
            started_ms: now,
            ended_ms: now,
            last_replayed_ms: None,
        };
        self.runs.insert(
            k,
            Run {
                replay,
                state: RunState::Starting,
                reason: None,
                archive: archive.name,
                bus: bus.to_owned(),
                only,
                cursor: None,
                due_ms: now + STARTING_MS,
                caller: (&r.call.caller).into(),
            },
        );
        Ok(Out::new()
            .with("ReplayArn", arn)
            .with("State", "STARTING")
            .with("ReplayStartTime", secs(now))
            .done())
    }

    /// Stops a replay that is `STARTING` or `RUNNING` (the only states the model lets one be cancelled in). It is `CANCELLING` until
    /// its next step, which is made due now.
    pub(crate) fn cancel_replay(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ReplayName");
        let k = key(r.account(), &[name]);
        let now = self.now_ms();
        let Some(run) = self.runs.get_mut(&k) else {
            if self.replays.get(&k).is_some() {
                return Err(not_cancellable(name));
            }
            return Err(no_replay(name));
        };
        if !matches!(run.state, RunState::Starting | RunState::Running) {
            return Err(not_cancellable(name));
        }
        run.state = RunState::Cancelling;
        run.due_ms = now;
        Ok(Out::new()
            .with("ReplayArn", run.replay.arn.clone())
            .with("State", "CANCELLING")
            .done())
    }

    /// The replay called `name`, from whichever table holds it.
    fn shown(&self, k: &str) -> Option<Shown<'_>> {
        if let Some(run) = self.runs.get(k) {
            return Some(Shown {
                p: &run.replay,
                state: run.state.as_str(),
                reason: run.reason.as_deref(),
                ended: !run.state.going(),
            });
        }
        self.replays.get(k).map(|p| Shown {
            p,
            state: "COMPLETED",
            reason: None,
            ended: true,
        })
    }

    pub(crate) fn describe_replay(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ReplayName");
        let s = self
            .shown(&key(r.account(), &[name]))
            .ok_or_else(|| no_replay(name))?;
        let p = s.p;
        Ok(Out::new()
            .with("ReplayName", p.name.clone())
            .with("ReplayArn", p.arn.clone())
            .maybe("Description", p.description.clone())
            .with("State", s.state)
            .maybe("StateReason", s.reason.map(str::to_owned))
            .with("EventSourceArn", p.source_arn.clone())
            .json("Destination", Some(&p.destination))
            .with("EventStartTime", secs(p.start_ms))
            .with("EventEndTime", secs(p.end_ms))
            .maybe("EventLastReplayedTime", p.last_replayed_ms.map(secs))
            .with("ReplayStartTime", secs(p.started_ms))
            .maybe("ReplayEndTime", s.ended.then(|| secs(p.ended_ms)))
            .done())
    }

    pub(crate) fn list_replays(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let source = r.opt("EventSourceArn");
        let state = r.opt("State");
        let mine = key(r.account(), &[""]);
        let mut keys: Vec<&str> = self
            .replays
            .with_prefix(&mine)
            .map(|(k, _)| k)
            .chain(self.runs.with_prefix(&mine).map(|(k, _)| k))
            .collect();
        keys.sort_unstable();
        let items = keys
            .into_iter()
            .filter_map(|k| self.shown(k).map(|s| (k.to_owned(), s)))
            .filter(|(_, s)| s.p.name.starts_with(prefix))
            .filter(|(_, s)| source.is_none_or(|x| s.p.source_arn == x))
            .filter(|(_, s)| state.is_none_or(|x| x == s.state));
        let (replays, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = replays
            .into_iter()
            .map(|s| {
                let p = s.p;
                Out::new()
                    .with("ReplayName", p.name.clone())
                    .with("EventSourceArn", p.source_arn.clone())
                    .with("State", s.state)
                    .maybe("StateReason", s.reason.map(str::to_owned))
                    .with("EventStartTime", secs(p.start_ms))
                    .with("EventEndTime", secs(p.end_ms))
                    .maybe("EventLastReplayedTime", p.last_replayed_ms.map(secs))
                    .with("ReplayStartTime", secs(p.started_ms))
                    .maybe("ReplayEndTime", s.ended.then(|| secs(p.ended_ms)))
                    .done()
            })
            .collect();
        Ok(with_token(Out::new().with("Replays", list).done(), token))
    }
}

fn no_replay(name: &str) -> ApiError {
    not_found(format!("Replay {name} does not exist."))
}

/// The model: "a replay can be canceled only when the state is Running or Starting". Words recorded 2026-11-19.
fn not_cancellable(name: &str) -> ApiError {
    ApiError::modeled(
        "IllegalStatusException",
        format!("Replay {name} is not in a valid state for this operation."),
    )
}

/// The rules a `Destination` limits the replay to (`FilterArns`): none means all of them.
fn filter_arns(destination: &Value) -> Vec<String> {
    destination
        .get("FilterArns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str().map(str::to_owned))
        .collect()
}

/// A time member of the request (epoch seconds) in milliseconds.
fn secs_of(r: &Req<'_>, name: &str) -> i64 {
    r.v.get(name)
        .and_then(Value::as_f64)
        .map_or(0, |s| (s * 1000.0).round() as i64)
}
