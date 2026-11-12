//! Replays: the archived events of a window put back on the archive's bus, through its rules (optionally only those named). A replay
//! finishes before `StartReplay` answers, so it is never running to be cancelled and `CancelReplay` is refused by name (SCOPE.md: no
//! asynchronous or cancellable replays). `StartReplay` answers `STARTING` as AWS does; the replay is `COMPLETED` when it is looked at.
//!
//! The wording of the refusals is from memory.

use aws_wire::ApiError;
use serde_json::Value;

use crate::archives::event_key;
use crate::core::Core;
use crate::event::Event;
use crate::model::{Replay, key};
use crate::names::{Parts, replay_arn};
use crate::page::{page, with_token};
use crate::req::{Req, exists, invalid, not_found};
use crate::util::{Out, secs};

/// The most events one replay reads.
const MAX_EVENTS: usize = 1_000_000;

/// What a replay is to put back, and where.
struct Plan<'a> {
    archive: &'a str,
    replay: &'a str,
    /// Start and end, in milliseconds.
    window: (i64, i64),
    bus: &'a str,
    /// Only the rules with these ARNs get the events; all of them when empty.
    only: &'a [String],
}

impl Core {
    pub(crate) async fn start_replay(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ReplayName");
        if self.replays.get(&key(r.account(), &[name])).is_some() {
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
        let only: Vec<String> = destination
            .get("FilterArns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str().map(str::to_owned))
            .collect();
        let bus = Parts::of(target).map(|p| p.tail).unwrap_or_default();
        let plan = Plan {
            archive: &archive.name,
            replay: name,
            window: (start, end),
            bus,
            only: &only,
        };
        let last = self.replay_events(r, &plan).await?;
        let arn = replay_arn(r.region(), r.account(), name);
        let now = self.now_ms();
        self.replays.insert(
            key(r.account(), &[name]),
            Replay {
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
                last_replayed_ms: last,
            },
        );
        Ok(Out::new()
            .with("ReplayArn", arn)
            .with("State", "STARTING")
            .with("ReplayStartTime", secs(now))
            .done())
    }

    /// Reads the archive's events of the window and sends each through the bus. The time of the last one is returned.
    async fn replay_events(
        &mut self,
        r: &Req<'_>,
        plan: &Plan<'_>,
    ) -> Result<Option<i64>, ApiError> {
        let (start, end) = plan.window;
        let from = event_key(r.account(), plan.archive, start, "");
        let to = event_key(r.account(), plan.archive, end, "");
        let rows = self
            .archived
            .scan(self.db.as_ref(), &from, &to, MAX_EVENTS, false)
            .map_err(|e| ApiError::internal(format!("could not read the archive: {e}")))?;
        let caller = r.call.caller.clone();
        let only = (!plan.only.is_empty()).then_some(plan.only);
        let mut last = None;
        for (_, bytes) in rows {
            let Some(mut event) = std::str::from_utf8(&bytes).ok().and_then(Event::of_text) else {
                continue;
            };
            event.replay_name = Some(plan.replay.to_owned());
            last = Some(event.time_ms);
            self.publish(&caller, r.account(), plan.bus, event, only)
                .await;
        }
        Ok(last)
    }

    pub(crate) fn describe_replay(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ReplayName");
        let p = self
            .replays
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| not_found(format!("Replay {name} does not exist.")))?;
        Ok(Out::new()
            .with("ReplayName", p.name.clone())
            .with("ReplayArn", p.arn.clone())
            .maybe("Description", p.description.clone())
            .with("State", "COMPLETED")
            .with("EventSourceArn", p.source_arn.clone())
            .json("Destination", Some(&p.destination))
            .with("EventStartTime", secs(p.start_ms))
            .with("EventEndTime", secs(p.end_ms))
            .maybe("EventLastReplayedTime", p.last_replayed_ms.map(secs))
            .with("ReplayStartTime", secs(p.started_ms))
            .with("ReplayEndTime", secs(p.ended_ms))
            .done())
    }

    pub(crate) fn list_replays(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let source = r.opt("EventSourceArn");
        let state = r.opt("State");
        let mine = key(r.account(), &[""]);
        let items = self
            .replays
            .with_prefix(&mine)
            .filter(|(_, p)| p.name.starts_with(prefix))
            .filter(|(_, p)| source.is_none_or(|s| p.source_arn == s))
            .filter(|_| state.is_none_or(|s| s == "COMPLETED"))
            .map(|(k, p)| (k.to_owned(), p));
        let (replays, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = replays
            .into_iter()
            .map(|p| {
                Out::new()
                    .with("ReplayName", p.name.clone())
                    .with("EventSourceArn", p.source_arn.clone())
                    .with("State", "COMPLETED")
                    .with("EventStartTime", secs(p.start_ms))
                    .with("EventEndTime", secs(p.end_ms))
                    .maybe("EventLastReplayedTime", p.last_replayed_ms.map(secs))
                    .with("ReplayStartTime", secs(p.started_ms))
                    .with("ReplayEndTime", secs(p.ended_ms))
                    .done()
            })
            .collect();
        Ok(with_token(Out::new().with("Replays", list).done(), token))
    }
}

/// A time member of the request (epoch seconds) in milliseconds.
fn secs_of(r: &Req<'_>, name: &str) -> i64 {
    r.v.get(name)
        .and_then(Value::as_f64)
        .map_or(0, |s| (s * 1000.0).round() as i64)
}
