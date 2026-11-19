//! Signals: what `AWS::CloudFormation::WaitCondition` waits for. A wait condition with a `CreationPolicy` counts the signals
//! `SignalResource` sends it (what `cfn-signal --stack --resource` calls); one with a `Handle` counts the signals `PUT` to its
//! handle's URL (what `cfn-signal <url>` and `curl -T` send). Either way it completes once it has `Count` success signals, fails at
//! the first failure signal, and fails when `Timeout` passes first.
//!
//! Signals are held in memory, by the resource or handle they are for, only while something can wait for them: a wait condition's
//! inbox lives while it waits; a handle's lives until the handle is deleted (signals sent before the wait condition starts are kept).
//! A restart forgets them, as it cuts off the operation that waited.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::sync::Notify;

use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::no_stack_plain;
use crate::model::Draft;
use crate::req::Req;

/// The path a wait condition handle's URL starts with: AWS's is a presigned URL in a bucket of this name.
pub(crate) const HANDLE_PREFIX: &str = "/cloudformation-waitcondition-";

/// A signal.
#[derive(Debug, Clone)]
pub(crate) struct Signal {
    pub(crate) success: bool,
    pub(crate) unique: String,
    pub(crate) data: String,
    pub(crate) reason: String,
}

/// What has arrived for one resource or handle.
#[derive(Debug, Default)]
struct Got {
    /// Success signals by unique id, with their data.
    success: BTreeMap<String, String>,
    failure: Option<Signal>,
}

#[derive(Debug, Default)]
struct Inbox {
    got: Mutex<Got>,
    notify: Notify,
}

static INBOXES: Mutex<Option<HashMap<String, Arc<Inbox>>>> = Mutex::new(None);

fn inbox(key: &str, make: bool) -> Option<Arc<Inbox>> {
    let mut g = INBOXES.lock().unwrap_or_else(PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    if make {
        return Some(Arc::clone(map.entry(key.to_owned()).or_default()));
    }
    map.get(key).cloned()
}

/// Starts keeping the signals for `key`.
pub(crate) fn open(key: &str) {
    drop(inbox(key, true));
}

/// Forgets the signals for `key`.
pub(crate) fn close(key: &str) {
    let mut g = INBOXES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(map) = g.as_mut() {
        map.remove(key);
    }
}

/// Hands a signal to whatever waits on `key`. `false` when nothing keeps signals for it.
pub(crate) fn deliver(key: &str, s: Signal) -> bool {
    let Some(b) = inbox(key, false) else {
        return false;
    };
    {
        let mut got = b.got.lock().unwrap_or_else(PoisonError::into_inner);
        if s.success {
            got.success.insert(s.unique.clone(), s.data.clone());
        } else if got.failure.is_none() {
            got.failure = Some(s);
        }
    }
    b.notify.notify_waiters();
    true
}

/// The inbox of a resource that waits for `SignalResource`.
pub(crate) fn resource_key(stack_id: &str, logical: &str) -> String {
    format!("resource|{stack_id}|{logical}")
}

/// The inbox of the handle whose URL carries `token`.
pub(crate) fn handle_key(token: &str) -> String {
    format!("handle|{token}")
}

/// A new wait condition handle's URL, its inbox opened. `endpoint` is where this process is reached.
pub(crate) fn handle_url(endpoint: &str, region: &str, stack_id: &str, logical: &str) -> String {
    let token = aws_seal::alphanumeric(40).unwrap_or_default();
    open(&handle_key(&token));
    let key = aws_core::uri::encode(format!("{stack_id}|{logical}").as_bytes(), false);
    format!(
        "{endpoint}{HANDLE_PREFIX}{region}/{key}?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Expires=86399&X-Amz-SignedHeaders=host&X-Amz-Signature={token}"
    )
}

/// The token a handle's URL carries.
pub(crate) fn token_of(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("X-Amz-Signature="))
        .map(str::to_owned)
}

/// The handle is deleted: its signals go.
pub(crate) fn close_handle(url: &str) {
    if let Some(t) = token_of(url) {
        close(&handle_key(&t));
    }
}

/// A `PUT` to a handle's URL: `{"Status": "SUCCESS"|"FAILURE", "UniqueId", "Data", "Reason"}`. `false` when the URL is not a live
/// handle's.
pub(crate) fn handle_put(url: &str, body: &[u8]) -> bool {
    let Some(token) = token_of(url) else {
        return false;
    };
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let s = Signal {
        // (memory) Anything but SUCCESS fails the wait condition.
        success: text("Status") == "SUCCESS",
        unique: text("UniqueId"),
        data: text("Data"),
        reason: text("Reason"),
    };
    deliver(&handle_key(&token), s)
}

/// How a wait ended without enough signals.
#[derive(Debug)]
pub(crate) enum Ended {
    Failed(Signal),
    TimedOut(usize),
    Cancelled,
}

/// Waits on `key` for `count` success signals, at most `timeout`. The signals' data by unique id.
pub(crate) async fn wait(
    stack_id: &str,
    key: &str,
    count: usize,
    timeout: Duration,
) -> Result<BTreeMap<String, String>, Ended> {
    let Some(b) = inbox(key, true) else {
        return Err(Ended::TimedOut(0));
    };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let notified = b.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let received = {
            let got = b.got.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(f) = &got.failure {
                return Err(Ended::Failed(f.clone()));
            }
            if got.success.len() >= count {
                return Ok(got.success.clone());
            }
            got.success.len()
        };
        tokio::select! {
            () = &mut notified => {}
            () = tokio::time::sleep_until(deadline) => return Err(Ended::TimedOut(received)),
            () = crate::cancel::cancelled(stack_id) => return Err(Ended::Cancelled),
        }
    }
}

/// An ISO 8601 duration of hours, minutes and seconds (`PT15M`, `PT1H30M`, `PT45S`), as a creation policy's `Timeout` is given.
pub(crate) fn iso_duration(s: &str) -> Option<Duration> {
    let rest = s.strip_prefix("PT")?;
    let mut secs: u64 = 0;
    let mut n = String::new();
    for c in rest.chars() {
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        let v: u64 = n.parse().ok()?;
        n.clear();
        secs += match c {
            'H' => v * 3600,
            'M' => v * 60,
            'S' => v,
            _ => return None,
        };
    }
    n.is_empty().then_some(Duration::from_secs(secs))
}

/// The data of the signals as the wait condition's `Data` attribute: a JSON object of unique id to data.
pub(crate) fn data_attribute(got: &BTreeMap<String, String>) -> String {
    let m: Map<String, Value> = got
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    Value::Object(m).to_string()
}

impl Core {
    pub(crate) fn signal_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self.find(name).filter(|s| s.status != "DELETE_COMPLETE") else {
            return Err(no_stack_plain(name));
        };
        let logical = r.s("LogicalResourceId");
        let Some(res) = stack.resources.get(logical) else {
            // Recorded 2026-11-19: AWS accepts a signal for a resource the stack does not have, and does nothing with it.
            return Ok(json!({}));
        };
        let (stack_id, ty, status) = (stack.id.clone(), res.ty.clone(), res.status.clone());
        let success = r.s("Status") == "SUCCESS";
        let unique = r.s("UniqueId").to_owned();
        let s = Signal {
            success,
            unique: unique.clone(),
            data: String::new(),
            reason: String::new(),
        };
        if !deliver(&resource_key(&stack_id, logical), s) {
            // Recorded 2026-11-19: a signal to a resource that no longer waits (its wait condition already complete) is
            // accepted and changes nothing.
            return Ok(json!({}));
        }
        // Recorded 2026-11-19: each signal shows as an event of the resource.
        let word = if success { "SUCCESS" } else { "FAILURE" };
        self.push_event(
            &stack_id,
            Draft {
                logical: logical.to_owned(),
                ty,
                status,
                reason: Some(format!("Received {word} signal with UniqueId {unique}")),
                ..Draft::default()
            },
        );
        Ok(json!({}))
    }
}
