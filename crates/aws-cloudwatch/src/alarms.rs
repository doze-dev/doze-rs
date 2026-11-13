//! Alarms as objects: put, describe, delete, force a state, switch their actions, and their tags.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::model::{Alarm, Kind, Spec};
use crate::req::{Put, Req, secs, validation};

/// Tags one resource may have.
const MAX_TAGS: usize = 50;
/// Alarms one region may hold (AWS: 5,000).
const MAX_ALARMS: usize = 5000;
/// What a new alarm's reason says (recorded: the state is `INSUFFICIENT_DATA`).
pub(crate) const UNCHECKED: &str = "Unchecked: Initial alarm creation";

pub(crate) fn alarm_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:cloudwatch:{region}:{account}:alarm:{name}")
}

/// `ResourceNotFound` with the empty message AWS answered with (recorded 2026-11-09).
pub(crate) fn no_alarm() -> ApiError {
    ApiError::modeled("ResourceNotFound", "")
}

impl Core {
    pub(crate) fn put_metric_alarm(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let math =
            r.v.get("Metrics")
                .is_some_and(|m| m.as_array().is_some_and(|a| !a.is_empty()));
        let spec = check_metric_alarm(r, math)?;
        let kind = if math { Kind::Math } else { Kind::Metric };
        self.store_alarm(r, kind, spec)
    }

    pub(crate) fn put_composite_alarm(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.store_alarm(r, Kind::Composite, Spec::default())
    }

    /// Makes or updates the alarm. An update replaces the configuration and leaves the state as it was.
    fn store_alarm(&mut self, r: &Req<'_>, kind: Kind, spec: Spec) -> Result<Value, ApiError> {
        let name = r.s("AlarmName").to_owned();
        let now = self.now_ms();
        let mut request = r.v.clone();
        if let Value::Object(m) = &mut request {
            m.remove("Tags");
        }
        let (old, created) = (self.alarms.get(&name), self.alarms.get(&name).is_none());
        if created && self.alarms.len() >= MAX_ALARMS {
            // (memory)
            return Err(ApiError::modeled(
                "LimitExceededFault",
                "The maximum number of alarms in this region has been reached.",
            ));
        }
        let mut alarm = Alarm {
            kind,
            request: request.to_string(),
            spec,
            tags: old.map_or_else(|| r.tags().into_iter().collect(), |a| a.tags.clone()),
            state: old.map_or_else(|| "INSUFFICIENT_DATA".to_owned(), |a| a.state.clone()),
            reason: old.map_or_else(|| first_reason(kind), |a| a.reason.clone()),
            reason_data: old.and_then(|a| a.reason_data.clone()),
            config_updated_ms: now,
            state_updated_ms: old.map_or(now, |a| a.state_updated_ms),
            transitioned_ms: old.map_or(now, |a| a.transitioned_ms),
            actions_enabled: r.flag("ActionsEnabled").unwrap_or(true),
            ok_actions: r.strings("OKActions"),
            alarm_actions: r.strings("AlarmActions"),
            insufficient_actions: r.strings("InsufficientDataActions"),
        };
        if kind != Kind::Metric && created {
            alarm.reason = first_reason(kind);
        }
        let summary = format!(
            "Alarm \"{name}\" {}",
            if created { "created" } else { "updated" }
        );
        let data = json!({
            "version": "1.0",
            "type": if created { "Create" } else { "Update" },
            "timestamp": iso(now),
            "configuration": request,
        })
        .to_string();
        let alarm_type = type_of(kind);
        self.alarms.insert(name.clone(), alarm);
        self.record(
            &name,
            now,
            ("ConfigurationUpdate", &summary, &data, alarm_type),
        );
        self.refresh_due(&name, false);
        Ok(json!({}))
    }

    pub(crate) fn describe_alarms(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let names = r.strings("AlarmNames");
        let prefix = r.opt("AlarmNamePrefix");
        let types = r.strings("AlarmTypes");
        let state = r.opt("StateValue");
        let action = r.opt("ActionPrefix");
        let after = r.opt("NextToken");
        let limit = r
            .int("MaxRecords")
            .map_or(50, |n| usize::try_from(n).unwrap_or(50));
        let all: Vec<(&str, &Alarm)> = self
            .alarms
            .iter()
            .filter(|(n, a)| {
                (names.is_empty() || names.iter().any(|x| x == n))
                    && prefix.is_none_or(|p| n.starts_with(p))
                    && (types.is_empty() || types.iter().any(|t| *t == type_of(a.kind)))
                    && state.is_none_or(|s| a.state == s)
                    && action.is_none_or(|p| {
                        [&a.ok_actions, &a.alarm_actions, &a.insufficient_actions]
                            .iter()
                            .any(|l| l.iter().any(|x| x.starts_with(p)))
                    })
                    && after.is_none_or(|t| *n > t)
            })
            .collect();
        let (mut metric, mut composite) = (Vec::new(), Vec::new());
        for (name, a) in all.iter().take(limit) {
            let shown = alarm_json(name, a, r.region(), r.account());
            if a.kind == Kind::Composite {
                composite.push(shown);
            } else {
                metric.push(shown);
            }
        }
        let mut out =
            json!({"MetricAlarms": metric, "CompositeAlarms": composite, "LogAlarms": []});
        if let (true, Some((last, _))) = (all.len() > limit, all.get(limit.saturating_sub(1))) {
            out.put("NextToken", json!(last));
        }
        Ok(out)
    }

    pub(crate) fn describe_alarms_for_metric(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (ns, metric, dims) = (r.s("Namespace"), r.s("MetricName"), r.dimensions());
        let (stat, ext, period, unit) = (
            r.opt("Statistic"),
            r.opt("ExtendedStatistic"),
            r.int("Period"),
            r.opt("Unit"),
        );
        let found: Vec<Value> = self
            .alarms
            .iter()
            .filter(|(_, a)| {
                a.kind == Kind::Metric
                    && a.spec.namespace == ns
                    && a.spec.metric == metric
                    && a.spec.dims == dims
                    && stat.is_none_or(|s| a.spec.stat == s)
                    && ext.is_none_or(|s| a.spec.stat == s)
                    && period.is_none_or(|p| a.spec.period_s == p)
                    && unit.is_none_or(|u| a.spec.unit.as_deref() == Some(u))
            })
            .map(|(n, a)| alarm_json(n, a, r.region(), r.account()))
            .collect();
        Ok(json!({ "MetricAlarms": found }))
    }

    /// Deleting an alarm that is not there is accepted (recorded 2026-11-09). Its history stays.
    pub(crate) fn delete_alarms(&mut self, r: &Req<'_>) -> Value {
        for name in r.strings("AlarmNames") {
            self.alarms.remove(&name);
            self.due.remove(&name);
        }
        json!({})
    }

    pub(crate) fn set_alarm_state(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("AlarmName").to_owned();
        let now = self.now_ms();
        let reason_data = r.opt("StateReasonData").map(str::to_owned);
        let alarm = self.alarms.get(&name).ok_or_else(no_alarm)?;
        let from = alarm.state.clone();
        self.move_to(
            &name,
            r.s("StateValue"),
            (r.s("StateReason"), reason_data.as_deref()),
            now,
            from != r.s("StateValue"),
        );
        // A forced state stands until the next evaluation, which is not before the next period.
        self.refresh_due(&name, true);
        Ok(json!({}))
    }

    pub(crate) fn alarm_actions_enabled(&mut self, r: &Req<'_>, on: bool) -> Value {
        for name in r.strings("AlarmNames") {
            if let Some(a) = self.alarms.get_mut(&name) {
                a.actions_enabled = on;
            }
        }
        json!({})
    }

    pub(crate) fn describe_alarm_history(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (name, kind) = (r.opt("AlarmName"), r.opt("HistoryItemType"));
        let (from, to) = (r.time("StartDate"), r.time("EndDate"));
        let types = r.strings("AlarmTypes");
        let mut items: Vec<(&str, &crate::model::Item)> = self
            .history
            .iter()
            .filter(|(n, _)| name.is_none_or(|x| x == *n))
            .flat_map(|(n, h)| h.items.iter().map(move |i| (n, i)))
            .filter(|(_, i)| {
                kind.is_none_or(|k| i.kind == k)
                    && from.is_none_or(|f| i.at_ms >= f)
                    && to.is_none_or(|t| i.at_ms <= t)
                    && (types.is_empty() || types.contains(&i.alarm_type))
            })
            .collect();
        items.sort_by_key(|(n, i)| (i.at_ms, n.to_owned()));
        if r.opt("ScanBy") != Some("TimestampAscending") {
            items.reverse();
        }
        let skip = r
            .opt("NextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = r
            .int("MaxRecords")
            .map_or(50, |n| usize::try_from(n).unwrap_or(50));
        let page: Vec<Value> = items
            .iter()
            .skip(skip)
            .take(limit)
            .map(|(n, i)| {
                json!({
                    "AlarmName": n,
                    "AlarmType": i.alarm_type,
                    "Timestamp": secs(i.at_ms),
                    "HistoryItemType": i.kind,
                    "HistorySummary": i.summary,
                    "HistoryData": i.data,
                })
            })
            .collect();
        let mut out = json!({ "AlarmHistoryItems": page });
        if items.len() > skip + limit {
            out.put("NextToken", json!((skip + limit).to_string()));
        }
        Ok(out)
    }

    /// The tags of the alarm or dashboard an ARN names.
    fn tags_of(&mut self, arn: &str) -> Result<&mut BTreeMap<String, String>, ApiError> {
        if let Some((_, name)) = arn.split_once(":alarm:") {
            return self
                .alarms
                .get_mut(name)
                .map(|a| &mut a.tags)
                .ok_or_else(not_found);
        }
        if let Some((_, name)) = arn.split_once(":dashboard/") {
            return self
                .docs
                .get_mut(&crate::model::doc_key("dashboard", name))
                .map(|d| &mut d.tags)
                .ok_or_else(not_found);
        }
        Err(not_found())
    }

    pub(crate) fn tag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        let tags = self.tags_of(r.s("ResourceARN"))?;
        let added = new.iter().filter(|(k, _)| !tags.contains_key(k)).count();
        if tags.len() + added > MAX_TAGS {
            // (memory)
            return Err(validation(format!(
                "A resource can have at most {MAX_TAGS} tags."
            )));
        }
        tags.extend(new);
        Ok(json!({}))
    }

    pub(crate) fn untag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys = r.strings("TagKeys");
        let tags = self.tags_of(r.s("ResourceARN"))?;
        for k in keys {
            tags.remove(&k);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_of(r.s("ResourceARN"))?;
        Ok(
            json!({"Tags": tags.iter().map(|(k, v)| json!({"Key": k, "Value": v})).collect::<Vec<_>>()}),
        )
    }
}

fn not_found() -> ApiError {
    // (memory)
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified resource was not found.",
    )
}

pub(crate) fn type_of(kind: Kind) -> &'static str {
    if kind == Kind::Composite {
        "CompositeAlarm"
    } else {
        "MetricAlarm"
    }
}

/// The reason a new alarm starts with. A metric alarm that is evaluated says what AWS says; the two kinds that are stored only
/// say so.
fn first_reason(kind: Kind) -> String {
    match kind {
        Kind::Metric => UNCHECKED.to_owned(),
        Kind::Math => {
            "Not evaluated locally: metric math is stored, not evaluated, in doze-aws.".to_owned()
        }
        Kind::Composite => {
            "Not evaluated locally: composite alarms are stored, not evaluated, in doze-aws."
                .to_owned()
        }
    }
}

/// An ISO 8601 time, as an alarm's history data writes one.
pub(crate) fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let base = aws_core::time::format_iso8601(secs);
    let millis = ms.rem_euclid(1000);
    format!("{}.{millis:03}+0000", base.trim_end_matches('Z'))
}

/// An alarm as `DescribeAlarms` shows it: what was sent, the defaults AWS fills in, and the state.
pub(crate) fn alarm_json(name: &str, a: &Alarm, region: &str, account: &str) -> Value {
    let mut v: Map<String, Value> = serde_json::from_str::<Value>(&a.request)
        .ok()
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default();
    v.remove("Tags");
    let mut out = Value::Object(v);
    out.put("AlarmName", json!(name));
    out.put("AlarmArn", json!(alarm_arn(region, account, name)));
    out.put("ActionsEnabled", json!(a.actions_enabled));
    out.put("OKActions", json!(a.ok_actions));
    out.put("AlarmActions", json!(a.alarm_actions));
    out.put("InsufficientDataActions", json!(a.insufficient_actions));
    out.put(
        "AlarmConfigurationUpdatedTimestamp",
        secs(a.config_updated_ms),
    );
    out.put("StateValue", json!(a.state));
    out.put("StateReason", json!(a.reason));
    if let Some(d) = &a.reason_data {
        out.put("StateReasonData", json!(d));
    }
    out.put("StateUpdatedTimestamp", secs(a.state_updated_ms));
    out.put("StateTransitionedTimestamp", secs(a.transitioned_ms));
    out
}

/// A metric alarm's request, held to the rules AWS holds it to (the messages are the recorded ones where they are marked so in
/// the tests, otherwise from memory), and read into what an evaluation needs.
fn check_metric_alarm(r: &Req<'_>, math: bool) -> Result<Spec, ApiError> {
    let (stat, ext) = (r.opt("Statistic"), r.opt("ExtendedStatistic"));
    if stat.is_some() && ext.is_some() {
        // Recorded on AWS (2026-11-09).
        return Err(validation(
            "PutMetricAlarm request can not be made with both Statistic and ExtendedStatistic parameters. Only one of them should be present",
        ));
    }
    let periods = r.int("EvaluationPeriods");
    let to_alarm = r.int("DatapointsToAlarm");
    if let (Some(n), Some(m)) = (periods, to_alarm)
        && m > n
    {
        // Recorded on AWS (2026-11-09).
        return Err(validation(
            "DatapointsToAlarm must be less than or equal to EvaluationPeriods",
        ));
    }
    let treat = r.opt("TreatMissingData").unwrap_or("missing");
    if !["breaching", "notBreaching", "ignore", "missing"].contains(&treat) {
        // Recorded on AWS (2026-11-09).
        return Err(validation(format!(
            "The value {treat} is not supported for TreatMissingData parameter. Supported values are [breaching, notBreaching, ignore, missing]."
        )));
    }
    if math {
        return Ok(Spec::default());
    }
    for (field, present) in [
        ("MetricName", r.opt("MetricName").is_some()),
        ("Namespace", r.opt("Namespace").is_some()),
        ("Period", r.int("Period").is_some()),
        ("EvaluationPeriods", periods.is_some()),
        ("Threshold", r.num("Threshold").is_some()),
        ("ComparisonOperator", r.opt("ComparisonOperator").is_some()),
        (
            "Statistic or ExtendedStatistic",
            stat.is_some() || ext.is_some(),
        ),
    ] {
        if !present {
            // (memory)
            return Err(validation(format!(
                "The parameter {field} is required for a metric alarm that has no Metrics."
            )));
        }
    }
    let n = periods.unwrap_or(1);
    Ok(Spec {
        namespace: r.s("Namespace").to_owned(),
        metric: r.s("MetricName").to_owned(),
        dims: r.dimensions(),
        stat: stat.or(ext).unwrap_or_default().to_owned(),
        unit: r.opt("Unit").map(str::to_owned),
        period_s: r.int("Period").unwrap_or(60),
        evaluation_periods: n,
        datapoints_to_alarm: to_alarm.unwrap_or(n),
        threshold: r.num("Threshold").unwrap_or(0.0),
        comparison: r.s("ComparisonOperator").to_owned(),
        treat_missing: treat.to_owned(),
    })
}
