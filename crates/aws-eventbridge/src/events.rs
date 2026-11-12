//! `PutEvents`: each entry is checked on its own (a bad one fails and the call still succeeds, recorded 2026-11-09), made into an
//! event, and sent through its bus (`route.rs`).

use aws_wire::ApiError;
use aws_wire::RequestId;
use serde_json::{Value, json};

use crate::core::Core;
use crate::event::Event;
use crate::names::{DEFAULT_BUS, Parts, bus_name};
use crate::req::{Req, invalid, no_bus};
use crate::util::Out;

/// The most an entire request may hold: the entries' sizes added up (documented).
const MAX_REQUEST_BYTES: usize = 256 * 1024;

/// An entry that cannot be accepted: the code and words of its line in the answer.
struct Rejected {
    code: &'static str,
    message: String,
}

fn argument(name: &str, reason: &str) -> Rejected {
    Rejected {
        code: "InvalidArgument",
        message: format!("Parameter {name} is not valid. Reason: {reason}"),
    }
}

/// The size EventBridge counts for an entry (documented): the time is 14 bytes, then the text members.
fn size_of(entry: &Value) -> usize {
    let text = |m: &str| entry.get(m).and_then(Value::as_str).map_or(0, str::len);
    let resources: usize = entry
        .get("Resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::len)
        .sum();
    let time = if entry.get("Time").is_some() { 14 } else { 0 };
    time + text("Source") + text("DetailType") + text("Detail") + resources
}

impl Core {
    pub(crate) async fn put_events(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let entries = r.list("Entries");
        if entries.iter().map(size_of).sum::<usize>() > MAX_REQUEST_BYTES {
            return Err(invalid(
                "Total size of the entries in the request is over the limit.",
            ));
        }
        let mut lines = Vec::with_capacity(entries.len());
        let mut failed = 0;
        for entry in entries {
            match self.put_one(r, entry).await? {
                Ok(id) => lines.push(json!({"EventId": id})),
                Err(rejected) => {
                    failed += 1;
                    lines.push(
                        json!({"ErrorCode": rejected.code, "ErrorMessage": rejected.message}),
                    );
                }
            }
        }
        Ok(Out::new()
            .with("FailedEntryCount", failed)
            .with("Entries", lines)
            .done())
    }

    /// One entry. The outer error fails the whole call (a bus that is not there); the inner one fails the entry.
    async fn put_one(
        &mut self,
        r: &Req<'_>,
        entry: &Value,
    ) -> Result<Result<String, Rejected>, ApiError> {
        let text = |m: &str| {
            entry
                .get(m)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        };
        let Some(source) = text("Source") else {
            return Ok(Err(argument("Source", "Source is a required argument.")));
        };
        let Some(detail_type) = text("DetailType") else {
            return Ok(Err(argument(
                "DetailType",
                "DetailType is a required argument.",
            )));
        };
        let Some(detail_text) = text("Detail") else {
            return Ok(Err(argument("Detail", "Detail is a required argument.")));
        };
        let Some(detail) = serde_json::from_str::<Value>(detail_text)
            .ok()
            .filter(Value::is_object)
        else {
            return Ok(Err(Rejected {
                code: "MalformedDetail",
                message: "Detail is malformed.".to_owned(),
            }));
        };
        let given = text("EventBusName").unwrap_or(DEFAULT_BUS);
        let (region, account) = self.bus_home(given, r);
        let bus = bus_name(given).ok_or_else(|| no_bus(given))?.to_owned();
        let now = self.now_ms();
        let time_ms = entry
            .get("Time")
            .and_then(Value::as_f64)
            .map_or(now, |s| (s * 1000.0).round() as i64);
        let event = Event {
            id: RequestId::new().to_string(),
            source: source.to_owned(),
            detail_type: detail_type.to_owned(),
            account: r.account().to_owned(),
            region: r.region().to_owned(),
            time_ms,
            resources: entry
                .get("Resources")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            detail,
            replay_name: None,
        };
        let id = event.id.clone();
        self.send_to_bus(&r.call.caller, &region, &account, &bus, event)
            .await?;
        Ok(Ok(id))
    }

    /// The region and account a bus name or ARN lives in: the caller's own, unless an ARN says otherwise.
    fn bus_home(&self, given: &str, r: &Req<'_>) -> (String, String) {
        match Parts::of(given) {
            Some(p) => (p.region.to_owned(), p.account.to_owned()),
            None => (r.region().to_owned(), r.account().to_owned()),
        }
    }
}
