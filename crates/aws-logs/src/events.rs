//! Writing events: `PutLogEvents`, the retention sweep, and the data points metric filters make.

use aws_wire::ApiError;
use doze_store::record;
use serde_json::{Value, json};

use crate::core::{Core, VERSION};
use crate::model::{Event, Stream, event_key, stream_key};
use crate::pattern::{self, Pattern};
use crate::req::{Put, Req, invalid, no_stream};

/// The most bytes a batch may hold: the messages in UTF-8 plus 26 for each event.
const MAX_BATCH_BYTES: usize = 1_048_576;
/// What each event counts as beyond its message.
pub(crate) const EVENT_OVERHEAD: usize = 26;
/// Events more than this far ahead of now are rejected.
const TOO_NEW_MS: i64 = 2 * 3_600_000;
/// Events older than this are rejected.
const TOO_OLD_MS: i64 = 14 * 86_400_000;
/// The longest span of time one batch may cover.
const MAX_SPAN_MS: i64 = 24 * 3_600_000;
/// A group's events past their retention are removed at most this often.
const SWEEP_EVERY_MS: i64 = 60_000;

/// The sequence token of a stream. PutLogEvents ignores the token it is given, so this only has to be there and move.
pub(crate) fn token_of(s: &Stream) -> String {
    format!(
        "{:056}",
        u128::from(s.no) * 1_000_000_000_000 + u128::from(s.written)
    )
}

/// One event of a request.
struct Incoming {
    ts: i64,
    message: String,
}

impl Core {
    pub(crate) fn put_events(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = self.group(r)?.name.clone();
        let key = stream_key(&group, r.s("logStreamName"));
        if self.streams.get(&key).is_none() {
            return Err(no_stream());
        }
        let events = incoming(&r.v)?;
        self.sweep(&group);
        let now = self.now_ms();
        let retention = self.groups.get(&group).and_then(|g| g.retention_days);
        let verdict = Verdict::of(&events, now, retention);
        let accepted = events
            .get(verdict.first_ok..verdict.end_ok)
            .unwrap_or_default();
        check_span(accepted)?;
        self.metric_points(&group, accepted);
        let stream = self.streams.get_mut(&key).ok_or_else(no_stream)?;
        for e in accepted {
            let bytes = record::encode(
                "event",
                VERSION,
                &Event {
                    ingested_ms: now,
                    message: e.message.clone(),
                },
            )
            .map_err(|x| ApiError::internal(x.to_string()))?;
            self.events
                .put(event_key(stream.no, e.ts, stream.written), bytes);
            stream.written += 1;
            stream.bytes += i64::try_from(e.message.len() + EVENT_OVERHEAD).unwrap_or(0);
            stream.first_ms = Some(stream.first_ms.map_or(e.ts, |f| f.min(e.ts)));
            stream.last_ms = Some(stream.last_ms.map_or(e.ts, |l| l.max(e.ts)));
        }
        if !accepted.is_empty() {
            stream.last_ingested_ms = Some(now);
        }
        let mut out = json!({"nextSequenceToken": token_of(stream)});
        if let Some(rejected) = verdict.rejected() {
            out.put("rejectedLogEventsInfo", rejected);
        }
        Ok(out)
    }

    /// Removes the events of `group` older than its retention (lazily: when the group is written or read, not more than once a
    /// minute).
    pub(crate) fn sweep(&mut self, group: &str) {
        let now = self.now_ms();
        let Some(days) = self.groups.get(group).and_then(|g| g.retention_days) else {
            return;
        };
        if self
            .swept
            .get(group)
            .is_some_and(|at| now - at < SWEEP_EVERY_MS)
        {
            return;
        }
        self.swept.insert(group.to_owned(), now);
        let cutoff = now - days * 86_400_000;
        let keys: Vec<String> = self
            .streams
            .with_prefix(&stream_key(group, ""))
            .map(|(k, _)| k.to_owned())
            .collect();
        for key in keys {
            let Some(no) = self.streams.get(&key).map(|s| s.no) else {
                continue;
            };
            let (from, to) = (event_key(no, 0, 0), event_key(no, cutoff, 0));
            let removed = self.sum_bytes(&from, &to);
            if removed == 0 && self.events_empty_before(&from, &to) {
                continue;
            }
            self.events.delete_range(&from, &to);
            if let Some(s) = self.streams.get_mut(&key) {
                s.bytes = (s.bytes - removed).max(0);
                if s.bytes == 0 {
                    s.first_ms = None;
                    s.last_ms = None;
                    s.last_ingested_ms = None;
                } else {
                    s.first_ms = s.first_ms.map(|f| f.max(cutoff));
                }
            }
        }
    }

    /// The bytes the events in `from..to` count as.
    fn sum_bytes(&self, from: &str, to: &str) -> i64 {
        let (mut total, mut at) = (0i64, from.to_owned());
        while let Ok(rows) = self.events.scan(self.db.as_ref(), &at, to, 500, false) {
            let Some((last, _)) = rows.last() else {
                break;
            };
            at = format!("{last}\u{0}");
            for (_, value) in &rows {
                if let Ok(e) = record::decode::<Event>("event", VERSION, value) {
                    total += i64::try_from(e.message.len() + EVENT_OVERHEAD).unwrap_or(0);
                }
            }
        }
        total
    }

    fn events_empty_before(&self, from: &str, to: &str) -> bool {
        self.events
            .scan(self.db.as_ref(), from, to, 1, false)
            .is_ok_and(|rows| rows.is_empty())
    }

    /// The data points the group's metric filters make of `events`, held until the call is answered.
    fn metric_points(&mut self, group: &str, events: &[Incoming]) {
        let Some(g) = self.groups.get(group) else {
            return;
        };
        for f in &g.metric_filters {
            let Ok(pattern) = pattern::parse(&f.pattern) else {
                continue;
            };
            let t = &f.transformation;
            let data: Vec<Value> = events
                .iter()
                .filter(|e| pattern.matches(&e.message))
                .filter_map(|e| datum(&pattern, t, e))
                .collect();
            if !data.is_empty() {
                self.emit
                    .push(json!({"Namespace": t.namespace, "MetricData": data}));
            }
        }
    }

    /// Hands the data points the last call made to CloudWatch, when it is there to take them.
    pub(crate) async fn send_metrics(&mut self, call: &aws_wire::service::Call) {
        let points = std::mem::take(&mut self.emit);
        let Some(peers) = self.peers.clone() else {
            return;
        };
        for body in points {
            if peers
                .call("logs", &call.caller, "monitoring", "PutMetricData", &body)
                .await
                .is_err()
            {
                // WHY: a metric filter's output is a side effect; the log events were accepted whether or not CloudWatch is there.
            }
        }
    }
}

/// A data point: the filter's value read from the event, at the event's time.
fn datum(pattern: &Pattern, t: &crate::model::Transformation, e: &Incoming) -> Option<Value> {
    let value: f64 = match t.value.parse() {
        Ok(v) => v,
        Err(_) => pattern.pick(&e.message, &t.value)?.parse().ok()?,
    };
    #[allow(clippy::cast_precision_loss)]
    let mut d = json!({
        "MetricName": t.name,
        "Value": value,
        "Timestamp": e.ts as f64 / 1000.0,
    });
    if let Some(u) = &t.unit {
        d.put("Unit", json!(u));
    }
    let dims: Vec<Value> = t
        .dimensions
        .iter()
        .filter_map(|(name, selector)| {
            let v = pattern.pick(&e.message, selector)?;
            Some(json!({"Name": name, "Value": v}))
        })
        .collect();
    if !dims.is_empty() {
        d.put("Dimensions", Value::Array(dims));
    }
    Some(d)
}

/// The events of a request, in the order sent, held to the size and order rules.
fn incoming(v: &Value) -> Result<Vec<Incoming>, ApiError> {
    let list = v
        .get("logEvents")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let events: Vec<Incoming> = list
        .iter()
        .map(|e| Incoming {
            ts: e.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
            message: e
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
        .collect();
    let total: usize = events
        .iter()
        .map(|e| e.message.len() + EVENT_OVERHEAD)
        .sum();
    if total > MAX_BATCH_BYTES {
        // (memory)
        return Err(invalid(format!(
            "Upload too large: {total} bytes exceeds limit of {MAX_BATCH_BYTES}"
        )));
    }
    // Recorded on AWS (2026-11-09).
    if events
        .windows(2)
        .any(|w| matches!(w, [a, b] if b.ts < a.ts))
    {
        return Err(invalid(
            "Log events in a single PutLogEvents request must be in chronological order.",
        ));
    }
    Ok(events)
}

/// A batch of valid events covers at most a day.
fn check_span(events: &[Incoming]) -> Result<(), ApiError> {
    match (events.first(), events.last()) {
        (Some(a), Some(b)) if b.ts - a.ts > MAX_SPAN_MS => {
            // (memory)
            Err(invalid(
                "The batch of log events in a single PutLogEvents request cannot span more than 24 hours.",
            ))
        }
        _ => Ok(()),
    }
}

/// Which events of a batch are taken: those from `first_ok` up to `end_ok`. The events are in time order, so the ones
/// refused are a run at each end.
struct Verdict {
    first_ok: usize,
    end_ok: usize,
    too_old_end: usize,
    expired_end: usize,
    too_new_start: Option<usize>,
}

impl Verdict {
    fn of(events: &[Incoming], now: i64, retention_days: Option<i64>) -> Self {
        let count = |keep: &dyn Fn(i64) -> bool| events.iter().take_while(|e| keep(e.ts)).count();
        let too_old_end = count(&|ts| ts < now - TOO_OLD_MS);
        let expired_end = retention_days.map_or(0, |d| count(&|ts| ts < now - d * 86_400_000));
        let too_new = events.iter().position(|e| e.ts > now + TOO_NEW_MS);
        Self {
            first_ok: too_old_end.max(expired_end),
            end_ok: too_new.unwrap_or(events.len()),
            too_old_end,
            expired_end,
            too_new_start: too_new,
        }
    }

    /// `rejectedLogEventsInfo`, when something was rejected. (The words of the three members are from the API's documentation.)
    fn rejected(&self) -> Option<Value> {
        let mut v = json!({});
        if self.too_old_end > 0 && self.too_old_end >= self.expired_end {
            v.put("tooOldLogEventEndIndex", json!(self.too_old_end));
        }
        if self.expired_end > 0 && self.expired_end > self.too_old_end {
            v.put("expiredLogEventEndIndex", json!(self.expired_end));
        }
        if let Some(at) = self.too_new_start {
            v.put("tooNewLogEventStartIndex", json!(at));
        }
        let empty = v.as_object().is_none_or(serde_json::Map::is_empty);
        (!empty).then_some(v)
    }
}
