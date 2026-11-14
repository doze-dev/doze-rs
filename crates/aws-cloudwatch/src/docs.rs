//! Documents that are stored and returned and applied to nothing: dashboards (the body is kept verbatim; nothing renders it) and
//! anomaly detectors (nothing models an anomaly).

use aws_model_cloudwatch::Operation;
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key};
use crate::req::{Put, Req, secs};

const DASHBOARD: &str = "dashboard";
const DETECTOR: &str = "anomaly-detector";
/// Dashboards one region may hold (AWS: 500 per account).
const MAX_DASHBOARDS: usize = 500;

fn dashboard_arn(account: &str, name: &str) -> String {
    format!("arn:aws:cloudwatch::{account}:dashboard/{name}")
}

impl Core {
    /// The tier-B families: stored, returned, not active locally (`stored.rs`).
    fn run_stored(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use crate::stored::{MUTE, RULE, STREAM};
        use Operation as O;
        match op {
            O::PutMetricStream => self.put_metric_stream(r),
            O::GetMetricStream => self.get_metric_stream(r),
            O::ListMetricStreams => Ok(self.list_metric_streams(r)),
            O::DeleteMetricStream => Ok(self.delete_stored(STREAM, r.s("Name"))),
            O::StartMetricStreams => Ok(self.set_states(STREAM, &r.strings("Names"), "running")),
            O::StopMetricStreams => Ok(self.set_states(STREAM, &r.strings("Names"), "stopped")),
            O::PutInsightRule => self.put_insight_rule(r),
            O::DescribeInsightRules => Ok(self.describe_insight_rules(r)),
            O::DeleteInsightRules => Ok(self.delete_insight_rules(r)),
            O::EnableInsightRules => Ok(self.set_states(RULE, &r.strings("RuleNames"), "ENABLED")),
            O::DisableInsightRules => {
                Ok(self.set_states(RULE, &r.strings("RuleNames"), "DISABLED"))
            }
            O::GetInsightRuleReport => self.get_insight_rule_report(r),
            O::PutAlarmMuteRule => self.put_alarm_mute_rule(r),
            O::GetAlarmMuteRule => self.get_alarm_mute_rule(r),
            O::ListAlarmMuteRules => Ok(self.list_alarm_mute_rules(r)),
            O::DeleteAlarmMuteRule => Ok(self.delete_stored(MUTE, r.s("AlarmMuteRuleName"))),
            _ => Err(crate::core::not_built()),
        }
    }

    pub(crate) fn run_docs(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        use Operation as O;
        match op {
            O::PutDashboard => self.put_dashboard(r),
            O::GetDashboard => self.get_dashboard(r),
            O::ListDashboards => Ok(self.list_dashboards(r)),
            O::DeleteDashboards => self.delete_dashboards(r),
            O::PutAnomalyDetector => self.put_detector(r),
            O::DescribeAnomalyDetectors => Ok(self.describe_detectors(r)),
            O::DeleteAnomalyDetector => self.delete_detector(r),
            other => self.run_stored(other, r),
        }
    }

    fn put_dashboard(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("DashboardName").to_owned();
        let valid_name = !name.is_empty()
            && name.len() <= 255
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !valid_name {
            // (memory)
            return Err(invalid_dashboard(
                "DashboardName",
                "The value for DashboardName is invalid: it may only contain letters, digits, '-' and '_'.",
            ));
        }
        let body = r.s("DashboardBody");
        if !serde_json::from_str::<Value>(body).is_ok_and(|v| v.is_object()) {
            // (memory)
            return Err(invalid_dashboard(
                "/",
                "Invalid JSON: the dashboard body must be a JSON object.",
            ));
        }
        let key = doc_key(DASHBOARD, &name);
        if self.docs.get(&key).is_none()
            && self.docs.with_prefix(&doc_key(DASHBOARD, "")).count() >= MAX_DASHBOARDS
        {
            // (memory)
            return Err(ApiError::modeled(
                "DashboardInvalidInputError",
                "The maximum number of dashboards has been reached.",
            ));
        }
        let tags = r.tags().into_iter().collect();
        let existing_tags = self.docs.get(&key).map(|d| d.tags.clone());
        let updated_ms = self.now_ms();
        self.docs.insert(
            key,
            Doc {
                json: body.to_owned(),
                updated_ms,
                tags: existing_tags.unwrap_or(tags),
            },
        );
        Ok(json!({"DashboardValidationMessages": []}))
    }

    fn get_dashboard(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("DashboardName");
        let d = self
            .docs
            .get(&doc_key(DASHBOARD, name))
            .ok_or_else(|| no_dashboard(name))?;
        Ok(json!({
            "DashboardArn": dashboard_arn(r.account(), name),
            "DashboardBody": d.json,
            "DashboardName": name,
        }))
    }

    fn list_dashboards(&self, r: &Req<'_>) -> Value {
        let prefix = r.opt("DashboardNamePrefix").unwrap_or_default();
        let base = doc_key(DASHBOARD, "");
        let entries: Vec<Value> = self
            .docs
            .with_prefix(&format!("{base}{prefix}"))
            .filter_map(|(k, d)| {
                let name = k.strip_prefix(&base)?;
                Some(json!({
                    "DashboardName": name,
                    "DashboardArn": dashboard_arn(r.account(), name),
                    "LastModified": secs(d.updated_ms),
                    "Size": d.json.len(),
                }))
            })
            .collect();
        json!({ "DashboardEntries": entries })
    }

    fn delete_dashboards(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let names = r.strings("DashboardNames");
        let missing: Vec<&String> = names
            .iter()
            .filter(|n| self.docs.get(&doc_key(DASHBOARD, n)).is_none())
            .collect();
        if !missing.is_empty() {
            // (memory)
            return Err(no_dashboard(
                &missing
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        for n in names {
            self.docs.remove(&doc_key(DASHBOARD, &n));
        }
        Ok(json!({}))
    }

    fn put_detector(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut request = r.v.clone();
        let wanted = detector_selector(&request);
        let existing = self
            .docs
            .with_prefix(&doc_key(DETECTOR, ""))
            .find(|(_, d)| {
                serde_json::from_str::<Value>(&d.json)
                    .is_ok_and(|v| detector_selector(&v) == wanted)
            })
            .map(|(k, _)| k.to_owned());
        let id = existing
            .as_deref()
            .and_then(|k| k.strip_prefix(&doc_key(DETECTOR, "")))
            .map(str::to_owned)
            .or_else(aws_seal::uuid_v4)
            .unwrap_or_default();
        request.put("AnomalyDetectorId", json!(id));
        request.put("StateValue", json!("PENDING_TRAINING"));
        let updated_ms = self.now_ms();
        self.docs.insert(
            doc_key(DETECTOR, &id),
            Doc {
                json: request.to_string(),
                updated_ms,
                tags: Default::default(),
            },
        );
        Ok(json!({"AnomalyDetectorId": id}))
    }

    fn describe_detectors(&self, r: &Req<'_>) -> Value {
        let ids = r.strings("AnomalyDetectorIds");
        let (ns, metric) = (r.opt("Namespace"), r.opt("MetricName"));
        let found: Vec<Value> = self
            .docs
            .with_prefix(&doc_key(DETECTOR, ""))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .filter(|v| {
                let text = |k: &str| v.get(k).and_then(Value::as_str);
                (ids.is_empty()
                    || ids
                        .iter()
                        .any(|i| Some(i.as_str()) == text("AnomalyDetectorId")))
                    && ns.is_none_or(|n| text("Namespace") == Some(n))
                    && metric.is_none_or(|m| text("MetricName") == Some(m))
            })
            .collect();
        json!({ "AnomalyDetectors": found })
    }

    fn delete_detector(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let wanted = detector_selector(&r.v);
        let victims: Vec<String> = self
            .docs
            .with_prefix(&doc_key(DETECTOR, ""))
            .filter(|(k, d)| {
                r.opt("AnomalyDetectorId")
                    .is_some_and(|id| *k == doc_key(DETECTOR, id))
                    || serde_json::from_str::<Value>(&d.json).is_ok_and(|v| {
                        r.opt("AnomalyDetectorId").is_none() && detector_selector(&v) == wanted
                    })
            })
            .map(|(k, _)| k.to_owned())
            .collect();
        if victims.is_empty() {
            // (memory)
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                "The specified anomaly detector does not exist.",
            ));
        }
        for k in victims {
            self.docs.remove(&k);
        }
        Ok(json!({}))
    }
}

/// What names a detector: the metric (old or new member layout) and the statistic.
fn detector_selector(v: &Value) -> Value {
    let metric = v.get("SingleMetricAnomalyDetector").unwrap_or(v);
    json!([
        metric.get("Namespace"),
        metric.get("MetricName"),
        metric.get("Dimensions"),
        metric.get("Stat"),
    ])
}

fn invalid_dashboard(path: &str, message: &str) -> ApiError {
    ApiError::modeled(
        "DashboardInvalidInputError",
        format!(
            "The dashboard body is invalid, there are 1 validation errors: [{{\"dataPath\":\"{path}\",\"message\":\"{message}\"}}]"
        ),
    )
}

fn no_dashboard(name: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "DashboardNotFoundError",
        format!("Dashboard named {name} does not exist."),
    )
}
