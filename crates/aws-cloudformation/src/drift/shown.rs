//! Drift results as the answers carry them (the model's `StackResourceDrift`, `DescribeStackDriftDetectionStatusOutput`,
//! `StackDriftInformation` and `StackResourceDriftInformation`).

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use super::{Detection, ResourceDrift};
use crate::describe::secs;
use crate::errors::validation;
use crate::j::J;
use crate::model::{Res, Stack};
use crate::req::Req;

/// A result as `StackResourceDrift`. A deleted resource has no expected or actual properties, and only a modified one has
/// differences (model).
pub(crate) fn resource_drift(d: &ResourceDrift) -> Value {
    let mut o = Map::new();
    o.insert("StackId".into(), d.stack_id.clone().into());
    o.insert("LogicalResourceId".into(), d.logical.clone().into());
    if let Some(p) = &d.physical {
        o.insert("PhysicalResourceId".into(), p.clone().into());
    }
    o.insert("ResourceType".into(), d.ty.clone().into());
    if d.status != "DELETED" {
        if let Some(e) = &d.expected {
            o.insert("ExpectedProperties".into(), e.clone().into());
        }
        if let Some(a) = &d.actual {
            o.insert("ActualProperties".into(), a.clone().into());
        }
    }
    if d.status == "MODIFIED" {
        let diffs: Vec<Value> = d
            .diffs
            .iter()
            .map(|x| {
                json!({"PropertyPath": x.path, "ExpectedValue": x.expected, "ActualValue": x.actual,
                    "DifferenceType": x.kind})
            })
            .collect();
        o.insert("PropertyDifferences".into(), Value::Array(diffs));
    }
    o.insert("StackResourceDriftStatus".into(), d.status.clone().into());
    o.insert("Timestamp".into(), secs(d.at_ms));
    if let Some(r) = &d.reason {
        o.insert("DriftStatusReason".into(), r.clone().into());
    }
    Value::Object(o)
}

/// A detection as `DescribeStackDriftDetectionStatus` answers it.
pub(crate) fn detection(d: &Detection) -> Value {
    let mut v = json!({
        "StackId": d.stack_id,
        "StackDriftDetectionId": d.id,
        "DetectionStatus": d.status,
        "Timestamp": secs(d.started_ms),
    });
    if let Some(s) = &d.stack_status {
        v.put("StackDriftStatus", s.clone().into());
    }
    if let Some(r) = &d.reason {
        v.put("DetectionStatusReason", r.clone().into());
    }
    if let Some(n) = d.drifted {
        v.put("DriftedStackResourceCount", n.into());
    }
    v
}

/// A page of results: `MaxResults` (1 to 100, the model's range) from the offset `NextToken` gives.
pub(crate) fn page(r: &Req<'_>, items: Vec<Value>) -> Value {
    let size = r
        .int("MaxResults")
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let start: usize = r.opt("NextToken").and_then(|t| t.parse().ok()).unwrap_or(0);
    let total = items.len();
    let taken: Vec<Value> = items.into_iter().skip(start).take(size).collect();
    let mut v = json!({ "StackResourceDrifts": taken });
    if start + size < total {
        v.put("NextToken", (start + size).to_string().into());
    }
    v
}

/// The stack's drift information: its latest finished detection, unless the stack was updated since.
pub(crate) fn stack_info<'a>(all: impl Iterator<Item = &'a Detection>, s: &Stack) -> Value {
    let since = s.updated_ms.unwrap_or(s.created_ms);
    let latest = all
        .filter(|d| d.stack_id == s.id && d.stack_status.is_some() && d.started_ms >= since)
        .max_by_key(|d| d.started_ms);
    match latest {
        Some(d) => {
            json!({"StackDriftStatus": d.stack_status, "LastCheckTimestamp": secs(d.started_ms)})
        }
        None => json!({"StackDriftStatus": "NOT_CHECKED"}),
    }
}

/// A resource's drift information: its latest result, unless the resource changed since.
pub(crate) fn resource_info(d: Option<&ResourceDrift>, res: &Res) -> Value {
    match d.filter(|d| d.at_ms >= res.updated_ms) {
        Some(d) => {
            json!({"StackResourceDriftStatus": d.status, "LastCheckTimestamp": secs(d.at_ms)})
        }
        None => json!({"StackResourceDriftStatus": "NOT_CHECKED"}),
    }
}

/// The refusal of `DetectStackResourceDrift` on a resource that is not checked. A type that is not checked (one doze-aws did not
/// make, or cannot read back) is refused in AWS's words for a type it does not check (recorded 2026-11-19). A resource not in a
/// complete state: (memory) the words.
pub(crate) fn not_supported(logical: &str, res: &Res) -> ApiError {
    if res.skipped || !super::read::readable(&res.ty) {
        // Recorded 2026-11-19.
        return validation(format!(
            "Drift detection is not supported for ResourceType {}",
            res.ty
        ));
    }
    validation(format!(
        "Drift detection is not supported for resource [{logical}] of type [{}]: it is not in a complete state",
        res.ty
    ))
}
