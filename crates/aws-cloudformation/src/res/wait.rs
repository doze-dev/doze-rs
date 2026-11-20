//! `AWS::CloudFormation::WaitCondition`: made by waiting for its signals (see `crate::signal`). With a `CreationPolicy`
//! (`ResourceSignal: {Count, Timeout}`, defaults 1 and `PT5M`) it counts `SignalResource` calls; otherwise its `Handle`, `Count`
//! (default 1) and `Timeout` (seconds, at most 43200) say which handle's signals it counts and for how long. The user guide's rules;
//! the words of the failures are (memory).

use std::time::Duration;

use serde_json::Value;

use super::{Done, Made, Rc, text};
use crate::signal::{self, Ended};

/// The longest a wait condition waits: 12 hours (user guide).
const LONGEST: u64 = 43_200;

/// What the wait condition waits for.
struct Wait {
    key: String,
    count: usize,
    timeout: Duration,
    /// Counting `SignalResource` calls (its inbox is the resource's own).
    by_policy: bool,
}

fn count_of(v: Option<&Value>) -> Option<usize> {
    match v? {
        Value::Number(n) => n.as_u64().and_then(|n| usize::try_from(n).ok()),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// A creation policy's `ResourceSignal`, if it has one.
pub(crate) fn resource_signal(creation: Option<&Value>) -> Option<&Value> {
    creation?.get("ResourceSignal")
}

fn plan(rc: &Rc, p: &Value, creation: Option<&Value>) -> Result<Wait, String> {
    if let Some(rs) = resource_signal(creation) {
        let timeout = match text(rs, "Timeout") {
            Some(t) => signal::iso_duration(&t)
                .ok_or_else(|| format!("Invalid Timeout {t} in the CreationPolicy: give an ISO 8601 duration such as PT15M"))?,
            None => Duration::from_secs(300),
        };
        return Ok(Wait {
            key: signal::resource_key(&rc.stack_id, &rc.logical),
            count: count_of(rs.get("Count")).unwrap_or(1),
            timeout: timeout.min(Duration::from_secs(LONGEST)),
            by_policy: true,
        });
    }
    let Some(handle) = text(p, "Handle") else {
        return Err("A WaitCondition needs a Handle and a Timeout, or a CreationPolicy".into());
    };
    let token = signal::token_of(&handle).ok_or_else(|| {
        format!("{handle} is not the URL of a wait condition handle of this stack")
    })?;
    let secs = text(p, "Timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .filter(|t| *t <= LONGEST)
        .ok_or_else(|| {
            "A WaitCondition's Timeout is a number of seconds, at most 43200".to_owned()
        })?;
    Ok(Wait {
        key: signal::handle_key(&token),
        count: count_of(p.get("Count")).unwrap_or(1),
        timeout: Duration::from_secs(secs),
        by_policy: false,
    })
}

/// Waits for the signals; the wait condition, its `Data` the signals' data by unique id.
pub(crate) async fn create(rc: &Rc, p: &Value, creation: Option<&Value>) -> Done {
    let w = plan(rc, p, creation)?;
    let got = signal::wait(&rc.stack_id, &w.key, w.count, w.timeout).await;
    if w.by_policy {
        signal::close(&w.key);
    }
    match got {
        // Recorded 2026-11-19: `SignalResource` carries no data, so a wait condition it completes has `Data` `{}`.
        Ok(_) if w.by_policy => {
            Ok(Made::new(super::generated_name(rc, 128, false)).with("Data", "{}"))
        }
        Ok(data) => Ok(Made::new(super::generated_name(rc, 128, false))
            .with("Data", signal::data_attribute(&data))),
        Err(Ended::Cancelled) => Err(crate::cancel::cut_off(true)),
        Err(Ended::Failed(s)) if w.by_policy => Err(format!(
            "Received FAILURE signal with UniqueId {}",
            s.unique
        )),
        Err(Ended::Failed(s)) => Err(format!(
            "WaitCondition received failed message: '{}' for uniqueId: {}",
            s.reason, s.unique
        )),
        Err(Ended::TimedOut(_)) if w.by_policy => Err(format!(
            "Failed to receive {} resource signal(s) within the specified duration",
            w.count
        )),
        Err(Ended::TimedOut(n)) => Err(format!(
            "WaitCondition timed out. Received {n} conditions when expecting {}",
            w.count
        )),
    }
}
