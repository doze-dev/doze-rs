//! Metric filters (they turn matching events into CloudWatch data points) and subscription filters (they forward matching events to a
//! Lambda function or a Kinesis stream: `forward.rs`).

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::groups::paged;
use crate::model::{MetricFilter, Subscription, Transformation};
use crate::pattern::{self, PatternError};
use crate::req::{Put, Req, invalid};

/// Metric filters one group may have (AWS: 100).
const MAX_METRIC_FILTERS: usize = 100;
/// Subscription filters one group may have (AWS: 2).
const MAX_SUBSCRIPTIONS: usize = 2;

/// A pattern that does not parse, in the words AWS uses.
pub(crate) fn refused(e: PatternError) -> ApiError {
    match e {
        PatternError::Invalid => invalid("Invalid filter pattern"),
        // (memory)
        PatternError::TooManyRegexes => ApiError::modeled(
            "LimitExceededException",
            "A filter pattern can contain at most two regular expressions.",
        ),
    }
}

/// A pattern that parses, and keeps the log group within five regular expressions across its metric and subscription filters
/// (`other` is how many the group's other filters hold).
fn checked(text: &str, other: usize) -> Result<(), ApiError> {
    let p = pattern::parse(text).map_err(refused)?;
    if p.regexes() > 0 && other + p.regexes() > pattern::MAX_REGEXES_PER_GROUP {
        // (memory)
        return Err(ApiError::modeled(
            "LimitExceededException",
            "A log group can have at most five regular expressions across its metric and subscription filters.",
        ));
    }
    Ok(())
}

/// How many regular expressions the group's filters hold, leaving out the metric or subscription filter named `except`.
fn regexes_besides(g: &crate::model::Group, except: (&str, &str)) -> usize {
    let metric = g
        .metric_filters
        .iter()
        .filter(|f| !(except.0 == "metric" && f.name == except.1))
        .map(|f| pattern::regexes_in(&f.pattern));
    let subs = g
        .subscriptions
        .iter()
        .filter(|f| !(except.0 == "subscription" && f.name == except.1))
        .map(|f| pattern::regexes_in(&f.pattern));
    metric.chain(subs).sum()
}

fn limit_exceeded() -> ApiError {
    // (memory)
    ApiError::modeled("LimitExceededException", "Resource limit exceeded.")
}

impl Core {
    pub(crate) fn put_metric_filter(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let other = self
            .group(r)
            .map_or(0, |g| regexes_besides(g, ("metric", r.s("filterName"))));
        checked(r.s("filterPattern"), other)?;
        let t =
            r.v.get("metricTransformations")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .ok_or_else(|| invalid("metricTransformations is required."))?;
        let text = |k: &str| {
            t.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let transformation = Transformation {
            name: text("metricName"),
            namespace: text("metricNamespace"),
            value: text("metricValue"),
            default: t.get("defaultValue").and_then(Value::as_f64),
            dimensions: t
                .get("dimensions")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                        .collect()
                })
                .unwrap_or_default(),
            unit: t.get("unit").and_then(Value::as_str).map(str::to_owned),
        };
        let filter = MetricFilter {
            name: r.s("filterName").to_owned(),
            pattern: r.s("filterPattern").to_owned(),
            transformation,
            created_ms: self.now_ms(),
            apply_on_transformed: r.flag("applyOnTransformedLogs").unwrap_or(false),
            field_selection: r.opt("fieldSelectionCriteria").map(str::to_owned),
            emit_system: r.strings("emitSystemFieldDimensions"),
        };
        let g = self.group_mut(r)?;
        if let Some(existing) = g.metric_filters.iter_mut().find(|f| f.name == filter.name) {
            let created = existing.created_ms;
            *existing = MetricFilter {
                created_ms: created,
                ..filter
            };
        } else if g.metric_filters.len() >= MAX_METRIC_FILTERS {
            return Err(limit_exceeded());
        } else {
            g.metric_filters.push(filter);
        }
        Ok(json!({}))
    }

    pub(crate) fn delete_metric_filter(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("filterName").to_owned();
        let g = self.group_mut(r)?;
        let before = g.metric_filters.len();
        g.metric_filters.retain(|f| f.name != name);
        if g.metric_filters.len() == before {
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                "The specified metric filter does not exist.",
            ));
        }
        Ok(json!({}))
    }

    pub(crate) fn describe_metric_filters(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let groups: Vec<&crate::model::Group> = match r.group_name() {
            Some(_) => vec![self.group(r)?],
            None => self.groups.iter().map(|(_, g)| g).collect(),
        };
        let prefix = r.opt("filterNamePrefix");
        let (metric, namespace) = (r.opt("metricName"), r.opt("metricNamespace"));
        let all: Vec<(&crate::model::Group, &MetricFilter)> = groups
            .iter()
            .flat_map(|g| g.metric_filters.iter().map(move |f| (*g, f)))
            .filter(|(_, f)| prefix.is_none_or(|p| f.name.starts_with(p)))
            .filter(|(_, f)| metric.is_none_or(|m| f.transformation.name == m))
            .filter(|(_, f)| namespace.is_none_or(|n| f.transformation.namespace == n))
            .collect();
        let refs: Vec<&(&crate::model::Group, &MetricFilter)> = all.iter().collect();
        let (page, token) = paged(&refs, r, 50, |(_, f)| f.name.as_str());
        let mut out = json!({
            "metricFilters": page.iter().map(|(g, f)| metric_json(g, f)).collect::<Vec<_>>(),
        });
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        Ok(out)
    }

    /// `TestMetricFilter` needs no group: it runs a pattern over some messages.
    pub(crate) fn test_metric_filter(r: &Req<'_>) -> Result<Value, ApiError> {
        let p = pattern::parse(r.s("filterPattern")).map_err(refused)?;
        let matches: Vec<Value> = r
            .strings("logEventMessages")
            .iter()
            .enumerate()
            .filter(|(_, m)| p.matches(m))
            .map(|(i, m)| {
                let extracted: serde_json::Map<String, Value> = p
                    .extracted(m)
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                json!({"eventNumber": i + 1, "eventMessage": m, "extractedValues": extracted})
            })
            .collect();
        Ok(json!({ "matches": matches }))
    }

    pub(crate) fn put_subscription(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let other = self.group(r).map_or(0, |g| {
            regexes_besides(g, ("subscription", r.s("filterName")))
        });
        checked(r.s("filterPattern"), other)?;
        let filter = Subscription {
            name: r.s("filterName").to_owned(),
            pattern: r.s("filterPattern").to_owned(),
            destination: r.s("destinationArn").to_owned(),
            role: r.opt("roleArn").map(str::to_owned),
            distribution: r.opt("distribution").map(str::to_owned),
            created_ms: self.now_ms(),
            apply_on_transformed: r.flag("applyOnTransformedLogs").unwrap_or(false),
            field_selection: r.opt("fieldSelectionCriteria").map(str::to_owned),
            emit_system: r.strings("emitSystemFields"),
        };
        let g = self.group_mut(r)?;
        if let Some(existing) = g.subscriptions.iter_mut().find(|f| f.name == filter.name) {
            let created = existing.created_ms;
            *existing = Subscription {
                created_ms: created,
                ..filter
            };
        } else if g.subscriptions.len() >= MAX_SUBSCRIPTIONS {
            return Err(limit_exceeded());
        } else {
            g.subscriptions.push(filter);
        }
        let (group, group_arn) = (g.name.clone(), g.arn(r.region(), r.account()));
        let origin = crate::forward::Origin {
            owner: r.account(),
            group: &group,
            stream: "",
            group_arn: &group_arn,
        };
        let now = self.now_ms();
        self.queue_control(&origin, r.s("destinationArn"), now);
        Ok(json!({}))
    }

    pub(crate) fn delete_subscription(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("filterName").to_owned();
        let g = self.group_mut(r)?;
        let before = g.subscriptions.len();
        g.subscriptions.retain(|f| f.name != name);
        if g.subscriptions.len() == before {
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                "The specified subscription filter does not exist.",
            ));
        }
        Ok(json!({}))
    }

    pub(crate) fn describe_subscriptions(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let g = self.group(r)?;
        let prefix = r.opt("filterNamePrefix");
        let all: Vec<&Subscription> = g
            .subscriptions
            .iter()
            .filter(|f| prefix.is_none_or(|p| f.name.starts_with(p)))
            .collect();
        let (page, token) = paged(&all, r, 50, |f| f.name.as_str());
        let mut out = json!({
            "subscriptionFilters": page.iter().map(|f| subscription_json(&g.name, f)).collect::<Vec<_>>(),
        });
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        Ok(out)
    }
}

fn metric_json(g: &crate::model::Group, f: &MetricFilter) -> Value {
    let t = &f.transformation;
    let mut tv = json!({
        "metricName": t.name,
        "metricNamespace": t.namespace,
        "metricValue": t.value,
    });
    if let Some(d) = t.default {
        tv.put("defaultValue", json!(d));
    }
    if !t.dimensions.is_empty() {
        tv.put("dimensions", json!(t.dimensions));
    }
    if let Some(u) = &t.unit {
        tv.put("unit", json!(u));
    }
    let mut v = json!({
        "filterName": f.name,
        "filterPattern": f.pattern,
        "metricTransformations": [tv],
        "creationTime": f.created_ms,
        "logGroupName": g.name,
        "applyOnTransformedLogs": f.apply_on_transformed,
    });
    if let Some(s) = &f.field_selection {
        v.put("fieldSelectionCriteria", json!(s));
    }
    if !f.emit_system.is_empty() {
        v.put("emitSystemFieldDimensions", json!(f.emit_system));
    }
    v
}

fn subscription_json(group: &str, f: &Subscription) -> Value {
    let mut v = json!({
        "filterName": f.name,
        "logGroupName": group,
        "filterPattern": f.pattern,
        "destinationArn": f.destination,
        "distribution": f.distribution.as_deref().unwrap_or("ByLogStream"),
        "creationTime": f.created_ms,
        "applyOnTransformedLogs": f.apply_on_transformed,
    });
    if let Some(r) = &f.role {
        v.put("roleArn", json!(r));
    }
    if let Some(s) = &f.field_selection {
        v.put("fieldSelectionCriteria", json!(s));
    }
    if !f.emit_system.is_empty() {
        v.put("emitSystemFields", json!(f.emit_system));
    }
    v
}
