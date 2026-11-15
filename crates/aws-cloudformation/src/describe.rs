//! The read operations: stacks, their events and resources, templates, exports and imports, stack policies and termination
//! protection; and the template checks (`ValidateTemplate`, `GetTemplateSummary`).

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::{no_stack, validation};
use crate::j::J;
use crate::model::{Event, Res, Stack};
use crate::req::Req;

/// A time as the answers carry it: CloudFormation writes milliseconds (`2026-11-12T10:00:00.123Z`), which the Query encoder
/// would drop from a number, so the text is handed over as it is to be written.
pub(crate) fn secs(ms: i64) -> Value {
    Value::String(crate::progress::iso_ms(ms))
}

/// How many items a page holds.
const PAGE: usize = 100;

fn put(o: &mut Map<String, Value>, k: &str, v: Option<impl Into<Value>>) {
    if let Some(v) = v {
        o.insert(k.to_owned(), v.into());
    }
}

/// A stack as `DescribeStacks` shows it.
pub(crate) fn stack_json(s: &Stack) -> Value {
    let mut o = Map::new();
    o.insert("StackId".into(), s.id.clone().into());
    o.insert("StackName".into(), s.name.clone().into());
    put(&mut o, "ChangeSetId", s.change_set_id.clone());
    put(&mut o, "Description", s.description.clone());
    if !s.params.is_empty() {
        o.insert(
            "Parameters".into(),
            Value::Array(crate::tmpl::params::described(&s.params)),
        );
    }
    o.insert("CreationTime".into(), secs(s.created_ms));
    put(&mut o, "DeletionTime", s.deleted_ms.map(secs));
    put(&mut o, "LastUpdatedTime", s.updated_ms.map(secs));
    o.insert("RollbackConfiguration".into(), json!({}));
    o.insert("StackStatus".into(), s.status.clone().into());
    put(&mut o, "StackStatusReason", s.reason.clone());
    o.insert("DisableRollback".into(), s.disable_rollback.into());
    o.insert(
        "DeploymentConfig".into(),
        json!({"DisableRollback": s.disable_rollback, "Mode": "STANDARD"}),
    );
    o.insert("NotificationARNs".into(), json!(s.notification_arns));
    put(&mut o, "TimeoutInMinutes", s.timeout);
    if !s.capabilities.is_empty() {
        o.insert("Capabilities".into(), json!(s.capabilities));
    }
    if !s.outputs.is_empty() {
        o.insert("Outputs".into(), outputs_json(&s.outputs));
    }
    put(&mut o, "RoleARN", s.role_arn.clone());
    o.insert(
        "Tags".into(),
        Value::Array(
            s.tags
                .iter()
                .map(|(k, v)| json!({"Key": k, "Value": v}))
                .collect(),
        ),
    );
    o.insert(
        "EnableTerminationProtection".into(),
        s.termination_protection.into(),
    );
    put(&mut o, "ParentId", s.parent_id.clone());
    put(&mut o, "RootId", s.root_id.clone());
    o.insert(
        "DriftInformation".into(),
        json!({"StackDriftStatus": "NOT_CHECKED"}),
    );
    if s.retain_except_on_create {
        o.insert("RetainExceptOnCreate".into(), true.into());
    }
    if let Some((id, ty)) = &s.last_op {
        o.insert(
            "LastOperations".into(),
            json!([{"OperationType": ty, "OperationId": id}]),
        );
    }
    Value::Object(o)
}

fn outputs_json(outputs: &[crate::model::Output]) -> Value {
    Value::Array(
        outputs
            .iter()
            .map(|x| {
                let mut v = json!({"OutputKey": x.key, "OutputValue": x.value});
                if let Some(d) = &x.description {
                    v.put("Description", d.clone().into());
                }
                if let Some(e) = &x.export {
                    v.put("ExportName", e.clone().into());
                }
                v
            })
            .collect(),
    )
}

fn event_json(e: &Event) -> Value {
    let mut o = Map::new();
    o.insert("StackId".into(), e.stack_id.clone().into());
    o.insert("EventId".into(), e.id.clone().into());
    o.insert("StackName".into(), e.stack_name.clone().into());
    put(&mut o, "OperationId", e.op_id.clone());
    o.insert("LogicalResourceId".into(), e.logical.clone().into());
    o.insert("PhysicalResourceId".into(), e.physical.clone().into());
    o.insert("ResourceType".into(), e.ty.clone().into());
    o.insert("Timestamp".into(), secs(e.at_ms));
    o.insert("ResourceStatus".into(), e.status.clone().into());
    put(&mut o, "ResourceStatusReason", e.reason.clone());
    put(&mut o, "ResourceProperties", e.props.clone());
    put(&mut o, "ClientRequestToken", e.token.clone());
    put(&mut o, "DetailedStatus", e.detailed.clone());
    Value::Object(o)
}

fn resource_common(logical: &str, r: &Res) -> Map<String, Value> {
    let mut o = Map::new();
    o.insert("LogicalResourceId".into(), logical.into());
    put(&mut o, "PhysicalResourceId", r.physical.clone());
    o.insert("ResourceType".into(), r.ty.clone().into());
    o.insert("ResourceStatus".into(), r.status.clone().into());
    put(&mut o, "ResourceStatusReason", r.reason.clone());
    o
}

/// A page of `items` from the `NextToken` the request gives.
fn page(r: &Req<'_>, items: Vec<Value>, key: &str) -> Value {
    let start: usize = r.opt("NextToken").and_then(|t| t.parse().ok()).unwrap_or(0);
    let total = items.len();
    let taken: Vec<Value> = items.into_iter().skip(start).take(PAGE).collect();
    let mut v = json!({ key: taken });
    if start + PAGE < total {
        v.put("NextToken", (start + PAGE).to_string().into());
    }
    v
}

impl Core {
    /// The stack a request names, or the recorded refusal.
    pub(crate) fn named(&self, r: &Req<'_>) -> Result<&Stack, ApiError> {
        let name = r.s("StackName");
        self.find(name).ok_or_else(|| no_stack(name))
    }

    pub(crate) fn describe_stacks(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        if r.opt("StackName").is_some() {
            return Ok(json!({"Stacks": [stack_json(self.named(r)?)]}));
        }
        let mut live: Vec<&Stack> = self
            .stacks
            .iter()
            .map(|(_, s)| s)
            .filter(|s| s.status != "DELETE_COMPLETE")
            .collect();
        live.sort_by_key(|s| std::cmp::Reverse(s.created_ms));
        Ok(page(
            r,
            live.into_iter().map(stack_json).collect(),
            "Stacks",
        ))
    }

    pub(crate) fn list_stacks(&self, r: &Req<'_>) -> Value {
        let filter = r.strings("StackStatusFilter");
        let mut all: Vec<&Stack> = self
            .stacks
            .iter()
            .map(|(_, s)| s)
            .filter(|s| {
                s.status != "REVIEW_IN_PROGRESS" || filter.iter().any(|f| f == "REVIEW_IN_PROGRESS")
            })
            .filter(|s| filter.is_empty() || filter.contains(&s.status))
            .collect();
        all.sort_by_key(|s| std::cmp::Reverse(s.created_ms));
        let items = all
            .into_iter()
            .map(|s| {
                let mut o = Map::new();
                o.insert("StackId".into(), s.id.clone().into());
                o.insert("StackName".into(), s.name.clone().into());
                put(&mut o, "TemplateDescription", s.description.clone());
                o.insert("CreationTime".into(), secs(s.created_ms));
                put(&mut o, "LastUpdatedTime", s.updated_ms.map(secs));
                put(&mut o, "DeletionTime", s.deleted_ms.map(secs));
                o.insert("StackStatus".into(), s.status.clone().into());
                put(&mut o, "StackStatusReason", s.reason.clone());
                put(&mut o, "ParentId", s.parent_id.clone());
                put(&mut o, "RootId", s.root_id.clone());
                o.insert(
                    "DriftInformation".into(),
                    json!({"StackDriftStatus": "NOT_CHECKED"}),
                );
                Value::Object(o)
            })
            .collect();
        page(r, items, "StackSummaries")
    }

    pub(crate) fn describe_stack_events(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        let prefix = format!("{}#", s.id);
        let mut events: Vec<&Event> = self.events.with_prefix(&prefix).map(|(_, e)| e).collect();
        events.reverse();
        Ok(page(
            r,
            events.into_iter().map(event_json).collect(),
            "StackEvents",
        ))
    }

    pub(crate) fn describe_stack_resource(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        let logical = r.s("LogicalResourceId");
        let Some(res) = s.resources.get(logical) else {
            // Recorded 2026-11-12.
            return Err(validation(format!(
                "Resource {logical} does not exist for stack {}",
                s.name
            )));
        };
        let mut o = resource_common(logical, res);
        o.insert("StackName".into(), s.name.clone().into());
        o.insert("StackId".into(), s.id.clone().into());
        o.insert("LastUpdatedTimestamp".into(), secs(res.updated_ms));
        o.insert(
            "Metadata".into(),
            res.metadata.clone().unwrap_or_else(|| "{}".into()).into(),
        );
        o.insert(
            "DriftInformation".into(),
            json!({"StackResourceDriftStatus": "NOT_CHECKED"}),
        );
        Ok(json!({"StackResourceDetail": o}))
    }

    pub(crate) fn describe_stack_resources(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let stack = match (r.opt("StackName"), r.opt("PhysicalResourceId")) {
            (Some(_), _) => self.named(r)?,
            (None, Some(p)) => self
                .stacks
                .iter()
                .map(|(_, s)| s)
                .find(|s| {
                    s.status != "DELETE_COMPLETE"
                        && s.resources
                            .values()
                            .any(|x| x.physical.as_deref() == Some(p))
                })
                // (memory)
                .ok_or_else(|| validation(format!("Stack for {p} does not exist")))?,
            // (memory)
            (None, None) => {
                return Err(validation(
                    "Either StackName or PhysicalResourceId must be specified.",
                ));
            }
        };
        let only = r.opt("LogicalResourceId");
        let items: Vec<Value> = stack
            .resources
            .iter()
            .filter(|(k, _)| only.is_none_or(|o| o == k.as_str()))
            .map(|(k, res)| {
                let mut o = resource_common(k, res);
                o.insert("StackName".into(), stack.name.clone().into());
                o.insert("StackId".into(), stack.id.clone().into());
                o.insert("Timestamp".into(), secs(res.updated_ms));
                o.insert(
                    "DriftInformation".into(),
                    json!({"StackResourceDriftStatus": "NOT_CHECKED"}),
                );
                Value::Object(o)
            })
            .collect();
        Ok(json!({"StackResources": items}))
    }

    pub(crate) fn list_stack_resources(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        let items: Vec<Value> = s
            .resources
            .iter()
            .map(|(k, res)| {
                let mut o = resource_common(k, res);
                o.insert("LastUpdatedTimestamp".into(), secs(res.updated_ms));
                o.insert(
                    "DriftInformation".into(),
                    json!({"StackResourceDriftStatus": "NOT_CHECKED"}),
                );
                Value::Object(o)
            })
            .collect();
        Ok(page(r, items, "StackResourceSummaries"))
    }

    pub(crate) fn get_template(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let processed = r.opt("TemplateStage") == Some("Processed");
        let (body, done) = match r.opt("ChangeSetName") {
            Some(cs) => {
                let c = self.change_set(r, cs)?;
                (
                    if processed {
                        c.processed.clone()
                    } else {
                        c.body.clone()
                    },
                    true,
                )
            }
            None => {
                let s = self.named(r)?;
                (
                    if processed {
                        s.processed.clone()
                    } else {
                        s.body.clone()
                    },
                    true,
                )
            }
        };
        let _ = done;
        Ok(json!({"TemplateBody": body, "StagesAvailable": ["Original", "Processed"]}))
    }

    pub(crate) fn list_exports(&self, r: &Req<'_>) -> Value {
        let items: Vec<Value> = self
            .exports()
            .into_iter()
            .map(|(name, (value, stack))| json!({"ExportingStackId": stack, "Name": name, "Value": value}))
            .collect();
        page(r, items, "Exports")
    }

    pub(crate) fn list_imports(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("ExportName");
        if !self.exports().contains_key(name) {
            // (memory)
            return Err(validation(format!("Export '{name}' does not exist.")));
        }
        let users: Vec<Value> = self
            .stacks
            .iter()
            .map(|(_, s)| s)
            .filter(|s| s.status != "DELETE_COMPLETE" && s.imports.iter().any(|i| i == name))
            .map(|s| Value::String(s.name.clone()))
            .collect();
        if users.is_empty() {
            // (memory)
            return Err(validation(format!(
                "Export '{name}' is not imported by any stack."
            )));
        }
        Ok(page(r, users, "Imports"))
    }

    pub(crate) async fn set_stack_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let id = self.named(r)?.id.clone();
        let policy = self
            .policy_text(r, "StackPolicyBody", "StackPolicyURL")
            .await?;
        if let Some(p) = &policy
            && serde_json::from_str::<Value>(p).is_err()
        {
            // (memory)
            return Err(validation(
                "Error validating stack policy: Invalid stack policy",
            ));
        }
        if let Some(s) = self.stacks.get_mut(&id) {
            s.policy = policy;
        }
        Ok(json!({}))
    }

    pub(crate) fn get_stack_policy(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        Ok(match &s.policy {
            Some(p) => json!({"StackPolicyBody": p}),
            None => json!({}),
        })
    }

    pub(crate) fn update_termination_protection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.named(r)?;
        if s.parent_id.is_some() {
            // (memory)
            return Err(validation(
                "Termination protection cannot be set on a nested stack; set it on the root stack.",
            ));
        }
        let id = s.id.clone();
        let on = r.flag("EnableTerminationProtection") == Some(true);
        if let Some(s) = self.stacks.get_mut(&id) {
            s.termination_protection = on;
        }
        Ok(json!({"StackId": id}))
    }

    pub(crate) async fn validate_template(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let body = self.template_text(r, None).await?;
        let t = crate::tmpl::read(&body)?;
        if crate::tmpl::transforms(&t).is_empty() {
            crate::tmpl::check(&t)?;
        }
        Ok(crate::tmpl::validate_answer(&t))
    }

    pub(crate) async fn get_template_summary(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let body = match r.opt("StackName") {
            Some(_) => self.named(r)?.body.clone(),
            None => self.template_text(r, None).await?,
        };
        let t = crate::tmpl::read(&body)?;
        if crate::tmpl::transforms(&t).is_empty() {
            crate::tmpl::check(&t)?;
        }
        Ok(crate::tmpl::summary_answer(&t, &t))
    }
}
