//! Usage plans metered: each request a REST method that requires an API key lets through is counted for the key on the usage plan
//! that puts the key on the stage, by UTC day (`usage` table, `<plan>/<key>`). The plan's quota (`DAY`, `WEEK`, `MONTH`, with its
//! offset) and throttle (a token bucket of `burstLimit` refilled at `rateLimit` a second, in memory) are enforced: 429 `Limit
//! Exceeded` (`QUOTA_EXCEEDED`) and 429 `Too Many Requests` (`THROTTLED`), the words `(memory)`. `GetUsage` reports `[used,
//! remaining]` per day (the model's shape) and `UpdateUsage` (`replace /remaining`) grants the rest of the current period.
//!
//! Counts are written with the next commit and need no timer. Days older than [`KEEP_DAYS`] are dropped when a key is counted.

use std::collections::HashMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::{Doc, child, text};
use crate::errors::{bad, not_found};
use crate::req::{Req, Res};

/// How many days of counts a key keeps (AWS's retention is not documented in the model: `(memory)`).
const KEEP_DAYS: i64 = 400;
const DAY_MS: i64 = 86_400_000;

/// What a key check answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Gate {
    /// The key opens the stage: its id. The request was counted.
    Open(String),
    /// No such key, not enabled, or in no usage plan for the stage.
    Closed,
    /// The plan's quota for the period is used up.
    Quota,
    /// The plan's throttle refused the request.
    Throttled,
}

/// The throttle buckets, by `<plan>/<key>`: tokens left and when they were counted. Made on a key's first throttled request.
#[derive(Debug, Default)]
pub(crate) struct Buckets(HashMap<String, (f64, i64)>);

impl Buckets {
    /// Takes a token from the bucket of `key`; `false` when there is none.
    fn take(&mut self, key: &str, rate: f64, burst: f64, now_ms: i64) -> bool {
        let cap = if burst >= 1.0 { burst } else { rate.max(1.0) };
        let slot = self.0.entry(key.to_owned()).or_insert((cap, now_ms));
        let secs = i32::try_from((now_ms - slot.1).clamp(0, 3_600_000)).unwrap_or(0);
        slot.0 = (slot.0 + f64::from(secs) / 1000.0 * rate).min(cap);
        slot.1 = now_ms;
        if slot.0 >= 1.0 {
            slot.0 -= 1.0;
            true
        } else {
            false
        }
    }

    pub(crate) fn clear(&mut self) {
        self.0 = HashMap::new();
    }
}

/// `YYYY-MM-DD` of a day number (days since 1970-01-01).
pub(crate) fn day_text(day: i64) -> String {
    let (y, m, d) = aws_core::time::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The day number of `YYYY-MM-DD`.
pub(crate) fn day_of(s: &str) -> Option<i64> {
    let mut it = s.splitn(3, '-');
    let y = it.next()?.parse::<i64>().ok()?;
    let m = it.next()?.parse::<i64>().ok()?;
    let d = it.next()?.parse::<i64>().ok()?;
    if s.len() != 10 || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let n = aws_core::time::days_from_civil(y, m, d);
    // 2026-02-30 is not a day: it must read back as itself.
    (day_text(n) == s).then_some(n)
}

/// The first day of the quota period holding `day`. Weeks begin on Monday and months on the 1st, each moved by the quota's offset
/// `(memory)`.
fn period_start(day: i64, quota: &Value) -> i64 {
    let offset = quota.get("offset").and_then(Value::as_i64).unwrap_or(0);
    match text(quota, "period").unwrap_or("DAY") {
        "WEEK" => day - (day - 4 - offset).rem_euclid(7),
        "MONTH" => {
            let (y, m, _) = aws_core::time::civil_from_days(day);
            let this = aws_core::time::days_from_civil(y, m, 1) + offset;
            if day >= this {
                this
            } else {
                let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
                aws_core::time::days_from_civil(py, pm, 1) + offset
            }
        }
        _ => day,
    }
}

fn used_on(rec: &Value, day: i64) -> i64 {
    rec.pointer(&format!("/days/{}", day_text(day)))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// What is left of the quota at the end of `day`: the limit and any grant for the period, less what the period used up to then.
fn remaining(rec: &Value, quota: Option<&Value>, day: i64) -> i64 {
    let Some(q) = quota else {
        return 0;
    };
    let limit = q.get("limit").and_then(Value::as_i64).unwrap_or(0);
    let start = period_start(day, q);
    let grant = rec
        .pointer(&format!("/grants/{}", day_text(start)))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let used: i64 = (start..=day).map(|d| used_on(rec, d)).sum();
    (limit + grant - used).max(0)
}

/// The plan (id and record) that puts key `key_id` on `api`'s `stage`.
fn plan_for<'a>(
    core: &'a Core,
    key_id: &str,
    api: &str,
    stage: &str,
) -> Option<(&'a str, &'a Value)> {
    core.plans.iter().map(|(id, p)| (id, &p.0)).find(|(_, p)| {
        let has_key = p
            .get("_keys")
            .and_then(Value::as_array)
            .is_some_and(|ks| ks.iter().any(|k| k.as_str() == Some(key_id)));
        let has_stage = p
            .get("apiStages")
            .and_then(Value::as_array)
            .is_some_and(|ss| {
                ss.iter()
                    .any(|s| text(s, "apiId") == Some(api) && text(s, "stage") == Some(stage))
            });
        has_key && has_stage
    })
}

impl Core {
    /// Whether the key whose value is `value` opens `api`'s `stage`; when it does, the request is counted against its plan.
    pub(crate) fn gate(&mut self, value: &str, api: &str, stage: &str, now_ms: i64) -> Gate {
        let Some(key_id) = self
            .keys
            .iter()
            .find(|(_, k)| {
                text(&k.0, "value") == Some(value)
                    && k.0.get("enabled").and_then(Value::as_bool) == Some(true)
            })
            .map(|(id, _)| id.to_owned())
        else {
            return Gate::Closed;
        };
        let Some((plan_id, plan)) = plan_for(self, &key_id, api, stage) else {
            return Gate::Closed;
        };
        let plan_id = plan_id.to_owned();
        let quota = plan.get("quota").cloned();
        let throttle = plan.get("throttle").cloned();
        let slot = format!("{plan_id}/{key_id}");
        let day = now_ms.div_euclid(DAY_MS);
        let empty = json!({});
        let rec = self.usage.get(&slot).map_or(&empty, |d| &d.0);
        if quota.is_some() && remaining(rec, quota.as_ref(), day) <= 0 {
            return Gate::Quota;
        }
        if let Some(t) = throttle
            .as_ref()
            .filter(|t| t.get("rateLimit").is_some() || t.get("burstLimit").is_some())
        {
            let rate = t.get("rateLimit").and_then(Value::as_f64).unwrap_or(0.0);
            let burst = t.get("burstLimit").and_then(Value::as_f64).unwrap_or(0.0);
            if !self.buckets.take(&slot, rate, burst, now_ms) {
                return Gate::Throttled;
            }
        }
        self.count_use(&slot, day);
        Gate::Open(key_id)
    }

    fn count_use(&mut self, slot: &str, day: i64) {
        let mut rec = self
            .usage
            .get(slot)
            .map_or_else(|| json!({}), |d| d.0.clone());
        let today = day_text(day);
        if let Some(days) = child(&mut rec, "days") {
            let n = days.get(&today).and_then(Value::as_i64).unwrap_or(0);
            days.insert(today, json!(n + 1));
            days.retain(|d, _| day_of(d).is_some_and(|d| day - d < KEEP_DAYS));
        }
        self.usage.insert(slot.to_owned(), Doc(rec));
    }
}

fn no_plan() -> ApiError {
    not_found("Invalid Usage Plan ID specified")
}

/// The keys of a plan, in id order.
fn plan_keys(plan: &Value) -> Vec<String> {
    let mut keys: Vec<String> = plan
        .get("_keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|k| k.as_str().map(str::to_owned))
        .collect();
    keys.sort();
    keys
}

/// `[used, remaining]` for each day from `from` to `to`.
fn daily(core: &Core, plan_id: &str, plan: &Value, key: &str, from: i64, to: i64) -> Value {
    let empty = json!({});
    let rec = core
        .usage
        .get(&format!("{plan_id}/{key}"))
        .map_or(&empty, |d| &d.0);
    let quota = plan.get("quota");
    Value::Array(
        (from..=to)
            .map(|d| json!([used_on(rec, d), remaining(rec, quota, d)]))
            .collect(),
    )
}

/// The longest range `GetUsage` reports, in days `(memory)`: AWS documents none in the model; this bounds the answer.
const MAX_RANGE: i64 = 366;

/// `GetUsage`.
pub(crate) fn get(core: &Core, r: &Req) -> Res {
    let plan_id = r.s("usagePlanId");
    let plan = core.plans.get(plan_id).map(|d| &d.0).ok_or_else(no_plan)?;
    let date = |k: &str| {
        day_of(r.s(k)).ok_or_else(|| {
            bad(format!(
                "Invalid {k}: '{}'. The date format is yyyy-MM-dd",
                r.s(k)
            ))
        })
    };
    let (from, to) = (date("startDate")?, date("endDate")?);
    if to < from {
        return Err(bad("The end date must not be before the start date"));
    }
    if to - from >= MAX_RANGE {
        return Err(bad(format!(
            "The date range may span at most {MAX_RANGE} days"
        )));
    }
    let mut keys = plan_keys(plan);
    if let Some(k) = r.opt("keyId") {
        keys.retain(|x| x == k);
    }
    let limit = r
        .int("limit")
        .and_then(|l| usize::try_from(l).ok())
        .filter(|l| *l > 0)
        .unwrap_or(25)
        .min(500);
    let at = r
        .opt("position")
        .and_then(|p| p.parse::<usize>().ok())
        .unwrap_or(0);
    let mut items = Map::new();
    for k in keys.iter().skip(at).take(limit) {
        items.insert(k.clone(), daily(core, plan_id, plan, k, from, to));
    }
    let mut out = json!({"usagePlanId": plan_id, "startDate": r.s("startDate"), "endDate": r.s("endDate"), "values": items});
    if at + limit < keys.len() {
        crate::doc::put(&mut out, "position", json!((at + limit).to_string()));
    }
    Ok(out.into())
}

/// `UpdateUsage`: `replace /remaining` sets what is left of the current period for the key.
pub(crate) fn update(core: &mut Core, r: &Req) -> Res {
    let plan_id = r.s("usagePlanId");
    let key = r.s("keyId");
    let plan = core
        .plans
        .get(plan_id)
        .map(|d| d.0.clone())
        .ok_or_else(no_plan)?;
    if !plan_keys(&plan).iter().any(|k| k == key) {
        return Err(not_found("Invalid Usage Plan Key identifier specified"));
    }
    let quota = plan.get("quota").cloned().ok_or_else(|| {
        bad("The usage plan has no quota, so there is no remaining quota to change")
    })?;
    let day = r.now_ms.div_euclid(DAY_MS);
    let slot = format!("{plan_id}/{key}");
    let mut rec = core
        .usage
        .get(&slot)
        .map_or_else(|| json!({}), |d| d.0.clone());
    for p in r.patches() {
        if p.path != "/remaining" || !matches!(p.op.as_str(), "replace" | "add") {
            return Err(bad(format!("Invalid patch path {}", p.path)));
        }
        let want = p
            .value
            .as_deref()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|v| *v >= 0)
            .ok_or_else(|| {
                bad(format!(
                    "Invalid value for /remaining: {}",
                    p.value.unwrap_or_default()
                ))
            })?;
        let now = remaining(&rec, Some(&quota), day);
        let start = day_text(period_start(day, &quota));
        if let Some(grants) = child(&mut rec, "grants") {
            let had = grants.get(&start).and_then(Value::as_i64).unwrap_or(0);
            grants.clear();
            grants.insert(start, json!(had + want - now));
        }
    }
    core.usage.insert(slot, Doc(rec));
    let today = day_text(day);
    let values = json!({key: daily(core, plan_id, &plan, key, day, day)});
    Ok(
        json!({"usagePlanId": plan_id, "startDate": today, "endDate": today, "values": values})
            .into(),
    )
}

/// Forgets the counts of a plan (deleted) or of a key (deleted, or taken out of a plan).
pub(crate) fn forget(core: &mut Core, plan: Option<&str>, key: Option<&str>) {
    let gone: Vec<String> = core
        .usage
        .iter()
        .map(|(k, _)| k.to_owned())
        .filter(|k| {
            let (p, kk) = k.split_once('/').unwrap_or((k, ""));
            plan.is_none_or(|x| x == p) && key.is_none_or(|x| x == kk)
        })
        .collect();
    for k in gone {
        core.usage.remove(&k);
    }
    core.buckets.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_begin_where_the_quota_says() {
        let day = day_of("2026-11-18").unwrap_or_default(); // a Wednesday
        assert_eq!(
            day_text(period_start(day, &json!({"period": "DAY"}))),
            "2026-11-18"
        );
        assert_eq!(
            day_text(period_start(day, &json!({"period": "WEEK"}))),
            "2026-11-16"
        );
        assert_eq!(
            day_text(period_start(day, &json!({"period": "WEEK", "offset": 3}))),
            "2026-11-12"
        );
        assert_eq!(
            day_text(period_start(day, &json!({"period": "MONTH"}))),
            "2026-11-01"
        );
        assert_eq!(
            day_text(period_start(day, &json!({"period": "MONTH", "offset": 20}))),
            "2026-10-21"
        );
        assert_eq!(day_of("2026-02-30"), None);
        assert_eq!(day_of("2026-1-01"), None);
    }

    #[test]
    fn remaining_counts_the_period_and_its_grant() {
        let q = json!({"period": "WEEK", "limit": 10});
        let day = day_of("2026-11-18").unwrap_or_default();
        let rec = json!({"days": {"2026-11-16": 3, "2026-11-18": 2, "2026-11-15": 9}, "grants": {"2026-11-16": 5}});
        assert_eq!(remaining(&rec, Some(&q), day), 10);
        assert_eq!(remaining(&rec, Some(&q), day - 1), 12);
    }

    #[test]
    fn a_bucket_lets_the_burst_through_then_refills() {
        let mut b = Buckets::default();
        assert!(b.take("k", 1.0, 2.0, 0));
        assert!(b.take("k", 1.0, 2.0, 0));
        assert!(!b.take("k", 1.0, 2.0, 0));
        assert!(b.take("k", 1.0, 2.0, 1000));
    }
}
