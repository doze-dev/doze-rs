//! Drift detection: `DetectStackDrift` (beside the actor, followed with `DescribeStackDriftDetectionStatus`),
//! `DetectStackResourceDrift` (at the call), `DescribeStackResourceDrifts`, and the drift information `DescribeStacks` and the
//! resource reads show. Each resource is read back from the service that keeps it (see [`read`]) and compared with the properties
//! its template set (see [`compare`]). A type with no reader, a resource not made locally (a skipped type) and one not in a
//! complete state are not checked: they keep `NOT_CHECKED` and are left out of the results, as the model says of the types AWS does
//! not support.
//!
//! What is kept: each detection (the last ten per stack), and each resource's latest result, in tables of their own (the stack
//! record keeps its on-disk shape). A result older than the resource's last change (an update since) shows as `NOT_CHECKED` in a
//! resource's drift information, and a detection older than the stack's last update does in the stack's.

mod compare;
mod read;
mod read_more;
mod shown;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use aws_wire::ApiError;

pub(crate) use compare::Diff;
use read::{Live, Probe};

use crate::core::Core;
use crate::errors::validation;
use crate::model::Res;
use crate::req::Req;

/// How many detections of one stack are kept.
const KEPT: usize = 10;

/// One stack drift detection.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Detection {
    pub(crate) id: String,
    pub(crate) stack_id: String,
    pub(crate) started_ms: i64,
    /// `DETECTION_IN_PROGRESS`, `DETECTION_COMPLETE` or `DETECTION_FAILED`.
    pub(crate) status: String,
    pub(crate) reason: Option<String>,
    /// `DRIFTED`, `IN_SYNC` or `UNKNOWN`, once complete.
    pub(crate) stack_status: Option<String>,
    pub(crate) drifted: Option<u32>,
}

/// One resource's latest drift result.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ResourceDrift {
    pub(crate) stack_id: String,
    pub(crate) logical: String,
    pub(crate) physical: Option<String>,
    pub(crate) ty: String,
    /// `IN_SYNC`, `MODIFIED`, `DELETED` or `UNKNOWN`.
    pub(crate) status: String,
    pub(crate) expected: Option<String>,
    pub(crate) actual: Option<String>,
    pub(crate) diffs: Vec<Diff>,
    pub(crate) at_ms: i64,
    pub(crate) reason: Option<String>,
}

/// A detection to run beside the actor.
#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) detection: String,
    pub(crate) stack_id: String,
    pub(crate) probe: Option<Probe>,
    pub(crate) resources: Vec<(String, Res)>,
    pub(crate) stack_tags: Vec<String>,
}

/// What a detection found.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) detection: String,
    pub(crate) drifts: Vec<ResourceDrift>,
}

/// The key of a resource's result.
fn drift_key(stack_id: &str, logical: &str) -> String {
    format!("{stack_id}#{logical}")
}

/// Whether drift detection checks the resource.
pub(crate) fn checkable(r: &Res) -> bool {
    read::readable(&r.ty)
        && !r.skipped
        && r.physical.is_some()
        && r.status.ends_with("_COMPLETE")
        && !r.status.starts_with("DELETE")
}

/// The stack's tags (CloudFormation puts them on its resources) and AWS's own `aws:` tags are not the template's.
fn strip_tags(actual: &mut Map<String, Value>, expected: &Value, stack_tags: &[String]) {
    let declared = |k: &str| -> bool {
        match expected.get("Tags") {
            Some(Value::Array(a)) => a
                .iter()
                .any(|t| t.get("Key").and_then(Value::as_str) == Some(k)),
            Some(Value::Object(o)) => o.contains_key(k),
            _ => false,
        }
    };
    let foreign =
        |k: &str| k.starts_with("aws:") || (stack_tags.iter().any(|s| s == k) && !declared(k));
    match actual.get_mut("Tags") {
        Some(Value::Array(a)) => {
            a.retain(|t| !foreign(t.get("Key").and_then(Value::as_str).unwrap_or_default()));
        }
        Some(Value::Object(o)) => o.retain(|k, _| !foreign(k)),
        _ => {}
    }
}

/// Checks one resource.
pub(crate) async fn check_one(
    probe: Option<&Probe>,
    stack_id: &str,
    logical: &str,
    res: &Res,
    stack_tags: &[String],
    now: i64,
) -> ResourceDrift {
    let physical = res.physical.clone().unwrap_or_default();
    let mut d = ResourceDrift {
        stack_id: stack_id.to_owned(),
        logical: logical.to_owned(),
        physical: res.physical.clone(),
        ty: res.ty.clone(),
        at_ms: now,
        ..ResourceDrift::default()
    };
    let got = match probe {
        Some(p) => read::read(p, &res.ty, &physical).await,
        None => Err("no service is running to read the resource from".into()),
    };
    let expected = res.props_value();
    match got {
        Ok(Live::Gone) => d.status = "DELETED".into(),
        Ok(Live::Found(mut actual)) => {
            strip_tags(&mut actual, &expected, stack_tags);
            let c = compare::compare(expected.as_object().unwrap_or(&Map::new()), &actual);
            d.status = if c.diffs.is_empty() {
                "IN_SYNC"
            } else {
                "MODIFIED"
            }
            .into();
            d.expected = Some(Value::Object(c.expected).to_string());
            d.actual = Some(Value::Object(c.actual).to_string());
            d.diffs = c.diffs;
        }
        Err(why) => {
            d.status = "UNKNOWN".into();
            d.reason = Some(format!("The resource could not be read: {why}"));
        }
    }
    d
}

/// Runs a detection beside the actor and reports what it found.
pub(crate) async fn run(runner: std::sync::Arc<crate::engine::Runner>, c: Check) {
    let mut drifts = Vec::new();
    for (logical, res) in &c.resources {
        let now = runner.clock.now_ms();
        drifts.push(
            check_one(
                c.probe.as_ref(),
                &c.stack_id,
                logical,
                res,
                &c.stack_tags,
                now,
            )
            .await,
        );
    }
    let found = Found {
        detection: c.detection,
        drifts,
    };
    if let Err(e) = runner
        .ask(crate::core::Request::Drift(Box::new(found)))
        .await
    {
        tracing::warn!("cloudformation: a drift result could not be recorded: {e}");
    }
}

impl Core {
    fn probe(&self, r: &Req<'_>) -> Option<Probe> {
        self.peers.clone().map(|peers| Probe {
            peers,
            caller: r.call.caller.clone(),
        })
    }

    /// The stack a drift operation names, if drift can be detected on it now.
    fn drift_target(&self, r: &Req<'_>) -> Result<crate::model::Stack, ApiError> {
        let s = self.named(r)?.clone();
        if s.status.ends_with("_IN_PROGRESS") {
            // (memory)
            return Err(validation(format!(
                "Drift detection cannot be performed on stack [{}] in status [{}]",
                s.name, s.status
            )));
        }
        Ok(s)
    }

    pub(crate) fn detect_stack_drift(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.drift_target(r)?;
        let only = r.strings("LogicalResourceIds");
        // Recorded 2026-11-19: AWS starts the detection when a named resource does not exist; it has nothing to check for it.
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let resources: Vec<(String, Res)> = s
            .resources
            .iter()
            .filter(|(k, res)| (only.is_empty() || only.contains(k)) && checkable(res))
            .map(|(k, res)| (k.clone(), res.clone()))
            .collect();
        let detection = Detection {
            id: id.clone(),
            stack_id: s.id.clone(),
            started_ms: self.tick(),
            status: "DETECTION_IN_PROGRESS".into(),
            ..Detection::default()
        };
        self.keep_detection(detection);
        self.checks.push(Check {
            detection: id.clone(),
            stack_id: s.id.clone(),
            probe: self.probe(r),
            resources,
            stack_tags: s.tags.iter().map(|(k, _)| k.clone()).collect(),
        });
        Ok(json!({"StackDriftDetectionId": id}))
    }

    /// Keeps a detection, and forgets the stack's oldest past [`KEPT`].
    fn keep_detection(&mut self, d: Detection) {
        let stack = d.stack_id.clone();
        self.detections.insert(d.id.clone(), d);
        let mut mine: Vec<(i64, String)> = self
            .detections
            .iter()
            .filter(|(_, x)| x.stack_id == stack)
            .map(|(k, x)| (x.started_ms, k.to_owned()))
            .collect();
        mine.sort();
        let extra = mine.len().saturating_sub(KEPT);
        for (_, k) in mine.into_iter().take(extra) {
            self.detections.remove(&k);
        }
    }

    /// What a detection found.
    pub(crate) fn drift_found(&mut self, f: Found) {
        let drifted = f
            .drifts
            .iter()
            .filter(|d| d.status == "MODIFIED" || d.status == "DELETED")
            .count();
        let unknown = f.drifts.iter().find(|d| d.status == "UNKNOWN").cloned();
        for d in f.drifts {
            self.drifts.insert(drift_key(&d.stack_id, &d.logical), d);
        }
        if let Some(det) = self.detections.get_mut(&f.detection) {
            det.drifted = Some(u32::try_from(drifted).unwrap_or(u32::MAX));
            det.stack_status = Some(
                match (drifted, &unknown) {
                    (0, Some(_)) => "UNKNOWN",
                    (0, None) => "IN_SYNC",
                    _ => "DRIFTED",
                }
                .into(),
            );
            match unknown {
                Some(u) => {
                    det.status = "DETECTION_FAILED".into();
                    // (memory)
                    det.reason = Some(format!(
                        "Failed to detect drift on resource [{}]: {}",
                        u.logical,
                        u.reason.unwrap_or_default()
                    ));
                }
                None => {
                    det.status = "DETECTION_COMPLETE".into();
                    det.reason = Some("Stack drift detection completed successfully".into());
                }
            }
        }
    }

    pub(crate) async fn detect_stack_resource_drift(
        &mut self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let s = self.drift_target(r)?;
        let logical = r.s("LogicalResourceId");
        let Some(res) = s.resources.get(logical) else {
            // Recorded for DescribeStackResource (2026-11-12); taken to be the same here.
            return Err(validation(format!(
                "Resource {logical} does not exist for stack {}",
                s.name
            )));
        };
        if !checkable(res) {
            return Err(shown::not_supported(logical, res));
        }
        let tags: Vec<String> = s.tags.iter().map(|(k, _)| k.clone()).collect();
        let now = self.tick();
        let d = check_one(self.probe(r).as_ref(), &s.id, logical, res, &tags, now).await;
        let shown = shown::resource_drift(&d);
        self.drifts.insert(drift_key(&s.id, logical), d);
        Ok(json!({"StackResourceDrift": shown}))
    }

    pub(crate) fn describe_stack_drift_detection_status(
        &self,
        r: &Req<'_>,
    ) -> Result<Value, ApiError> {
        let id = r.s("StackDriftDetectionId");
        let Some(d) = self.detections.get(id) else {
            // (memory)
            return Err(validation(format!("Drift detection [{id}] does not exist")));
        };
        Ok(shown::detection(d))
    }

    pub(crate) fn describe_stack_resource_drifts(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        let filter = r.strings("StackResourceDriftStatusFilters");
        let prefix = format!("{}#", s.id);
        let items: Vec<Value> = self
            .drifts
            .with_prefix(&prefix)
            .map(|(_, d)| d)
            .filter(|d| filter.is_empty() || filter.contains(&d.status))
            .map(shown::resource_drift)
            .collect();
        Ok(shown::page(r, items))
    }

    /// The drift information of a stack, for the stack reads.
    pub(crate) fn stack_drift_info(&self, s: &crate::model::Stack) -> Value {
        shown::stack_info(self.detections.iter().map(|(_, d)| d), s)
    }

    /// The drift information of a resource, for the resource reads.
    pub(crate) fn resource_drift_info(&self, stack_id: &str, logical: &str, res: &Res) -> Value {
        shown::resource_info(self.drifts.get(&drift_key(stack_id, logical)), res)
    }

    /// Forgets a stack's drift results (its record went).
    pub(crate) fn forget_drift(&mut self, stack_id: &str) {
        let keys: Vec<String> = self
            .detections
            .iter()
            .filter(|(_, d)| d.stack_id == stack_id)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in keys {
            self.detections.remove(&k);
        }
        let prefix = format!("{stack_id}#");
        let keys: Vec<String> = self
            .drifts
            .with_prefix(&prefix)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in keys {
            self.drifts.remove(&k);
        }
    }
}
