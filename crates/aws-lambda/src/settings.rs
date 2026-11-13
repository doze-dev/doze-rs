//! What is set on a function beside its configuration: reserved and provisioned concurrency, asynchronous invocation settings, tags,
//! runtime management and recursion, and the account's limits. Reserved concurrency is honoured by the runner; provisioned
//! concurrency, runtime management and recursion are accepted and stored (nothing is pre-warmed or updated locally).

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::{Core, Out};
use crate::errors::{invalid, not_found};
use crate::functions::{LATEST, page};
use crate::req::Req;

/// Lambda's default account concurrency, of which 100 must stay unreserved.
const ACCOUNT_CONCURRENCY: i64 = 1000;
const MIN_UNRESERVED: i64 = 100;

impl Core {
    pub(crate) fn put_concurrency(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find(r, &f)?;
        let n = r.int("ReservedConcurrentExecutions").unwrap_or(0);
        let others: i64 = self
            .functions
            .iter()
            .filter(|(k, _)| *k != f.name)
            .filter_map(|(_, f)| f.reserved)
            .sum();
        if ACCOUNT_CONCURRENCY - others - n < MIN_UNRESERVED {
            return Err(invalid(format!(
                "Specified ReservedConcurrentExecutions for function decreases account's UnreservedConcurrentExecution below its minimum value of [{MIN_UNRESERVED}]."
            )));
        }
        self.find_mut(r, &f)?.reserved = Some(n);
        Ok(json!({"ReservedConcurrentExecutions": n}).into())
    }

    pub(crate) fn get_concurrency(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        Ok(function
            .reserved
            .map_or(json!({}), |n| json!({"ReservedConcurrentExecutions": n}))
            .into())
    }

    pub(crate) fn delete_concurrency(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find_mut(r, &f)?.reserved = None;
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    fn provisioned_view(&self, r: &Req, name: &str, qualifier: &str, stored: &str) -> Value {
        let cfg: Value = serde_json::from_str(stored).unwrap_or(Value::Null);
        let n = cfg
            .get("ProvisionedConcurrentExecutions")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        json!({
            "RequestedProvisionedConcurrentExecutions": n,
            "AvailableProvisionedConcurrentExecutions": n,
            "AllocatedProvisionedConcurrentExecutions": n,
            "Status": "READY",
            "StatusReason": "Accepted by doze-aws: nothing is pre-warmed locally.",
            "LastModified": cfg.get("LastModified").cloned().unwrap_or(Value::Null),
            "FunctionArn": self.arn(r, name, Some(qualifier)),
        })
    }

    fn provisioned_qualifier(r: &Req) -> Result<String, ApiError> {
        let q = r.function()?.qualifier;
        match q {
            Some(q) if q != LATEST => Ok(q),
            _ => Err(invalid(
                "Provisioned concurrency needs a version or alias qualifier.",
            )),
        }
    }

    pub(crate) fn put_provisioned(&mut self, r: &Req) -> Result<Out, ApiError> {
        let q = Self::provisioned_qualifier(r)?;
        let f = r.function()?;
        let function = self.find(r, &f)?;
        self.snap_of(r, function, Some(&q))?;
        let stored = json!({
            "ProvisionedConcurrentExecutions": r.int("ProvisionedConcurrentExecutions").unwrap_or(0),
            "LastModified": crate::config::stamp(self.now_ms()),
        })
        .to_string();
        let view = self.provisioned_view(r, &f.name, &q, &stored);
        self.find_mut(r, &f)?.provisioned.insert(q, stored);
        Ok(Out {
            json: view,
            status: Some(202),
        })
    }

    pub(crate) fn get_provisioned(&self, r: &Req) -> Result<Out, ApiError> {
        let q = Self::provisioned_qualifier(r)?;
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let stored = function.provisioned.get(&q).ok_or_else(|| {
            not_found(format!(
                "No Provisioned Concurrency Config found for this function: {}",
                self.arn(r, &f.name, Some(&q))
            ))
        })?;
        Ok(self.provisioned_view(r, &f.name, &q, stored).into())
    }

    pub(crate) fn delete_provisioned(&mut self, r: &Req) -> Result<Out, ApiError> {
        let q = Self::provisioned_qualifier(r)?;
        let f = r.function()?;
        let there = self.find_mut(r, &f)?.provisioned.remove(&q).is_some();
        if !there {
            return Err(not_found(
                "No Provisioned Concurrency Config found for this function.",
            ));
        }
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn list_provisioned(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let rows = function
            .provisioned
            .iter()
            .map(|(q, s)| self.provisioned_view(r, &f.name, q, s))
            .collect();
        Ok(page(r, rows, "ProvisionedConcurrencyConfigs", |v| {
            v.get("FunctionArn")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .into())
    }

    fn event_invoke_key(f: &crate::arn::FnRef) -> String {
        f.qualifier.clone().unwrap_or_else(|| LATEST.to_owned())
    }

    fn event_invoke_view(&self, r: &Req, name: &str, key: &str, stored: &str) -> Value {
        let mut cfg: Map<String, Value> = serde_json::from_str(stored).unwrap_or_default();
        cfg.insert(
            "FunctionArn".into(),
            json!(self.arn(r, name, (key != LATEST).then_some(key))),
        );
        Value::Object(cfg)
    }

    pub(crate) fn put_event_invoke(&mut self, r: &Req, merge: bool) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let key = Self::event_invoke_key(&f);
        self.snap_of(r, function, f.qualifier.as_deref())?;
        let mut cfg: Map<String, Value> = if merge {
            let current = function.event_invoke.get(&key).ok_or_else(|| {
                not_found(format!(
                    "The function {} doesn't have an EventInvokeConfig",
                    self.arn(r, &f.name, f.qualifier.as_deref())
                ))
            })?;
            serde_json::from_str(current).unwrap_or_default()
        } else {
            Map::new()
        };
        for k in [
            "MaximumRetryAttempts",
            "MaximumEventAgeInSeconds",
            "DestinationConfig",
        ] {
            if let Some(v) = r.value(k) {
                cfg.insert(k.into(), v.clone());
            }
        }
        cfg.insert("LastModified".into(), json!(self.now_ms() / 1000));
        let stored = Value::Object(cfg).to_string();
        let view = self.event_invoke_view(r, &f.name, &key, &stored);
        self.find_mut(r, &f)?.event_invoke.insert(key, stored);
        Ok(view.into())
    }

    pub(crate) fn get_event_invoke(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let key = Self::event_invoke_key(&f);
        let stored = function.event_invoke.get(&key).ok_or_else(|| {
            not_found(format!(
                "The function {} doesn't have an EventInvokeConfig",
                self.arn(r, &f.name, f.qualifier.as_deref())
            ))
        })?;
        Ok(self.event_invoke_view(r, &f.name, &key, stored).into())
    }

    pub(crate) fn delete_event_invoke(&mut self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let key = Self::event_invoke_key(&f);
        let gone = self.find_mut(r, &f)?.event_invoke.remove(&key).is_some();
        if !gone {
            return Err(not_found(format!(
                "The function {} doesn't have an EventInvokeConfig",
                self.arn(r, &f.name, f.qualifier.as_deref())
            )));
        }
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn list_event_invoke(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let rows = function
            .event_invoke
            .iter()
            .map(|(k, s)| self.event_invoke_view(r, &f.name, k, s))
            .collect();
        Ok(page(r, rows, "FunctionEventInvokeConfigs", |v| {
            v.get("FunctionArn")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .into())
    }

    /// The function or mapping a tagging ARN names.
    fn tag_target(&mut self, r: &Req) -> Result<Tagged, ApiError> {
        let resource = r.text("Resource");
        if resource.contains(":event-source-mapping:") {
            let uuid = resource.rsplit(':').next().unwrap_or_default();
            return self
                .mappings
                .get(uuid)
                .map(|_| Tagged::Mapping(uuid.to_owned()))
                .ok_or_else(|| {
                    not_found(format!(
                        "The resource you requested does not exist: {resource}"
                    ))
                });
        }
        let f = crate::arn::parse(resource, None, r.region(), r.account())?;
        self.find(r, &f)?;
        Ok(Tagged::Function(f.name))
    }

    pub(crate) fn tag_resource(&mut self, r: &Req) -> Result<Out, ApiError> {
        let tags = crate::functions::tags_of(r.value("Tags"));
        match self.tag_target(r)? {
            Tagged::Function(n) => self.functions.get_mut(&n).map(|f| f.tags.extend(tags)),
            Tagged::Mapping(u) => self.mappings.get_mut(&u).map(|m| m.tags.extend(tags)),
        };
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn untag_resource(&mut self, r: &Req) -> Result<Out, ApiError> {
        let keys: Vec<String> = r
            .value("tagKeys")
            .or_else(|| r.value("TagKeys"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|k| k.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        match self.tag_target(r)? {
            Tagged::Function(n) => self
                .functions
                .get_mut(&n)
                .map(|f| f.tags.retain(|k, _| !keys.contains(k))),
            Tagged::Mapping(u) => self
                .mappings
                .get_mut(&u)
                .map(|m| m.tags.retain(|k, _| !keys.contains(k))),
        };
        Ok(Out {
            json: json!({}),
            status: Some(204),
        })
    }

    pub(crate) fn list_tags(&mut self, r: &Req) -> Result<Out, ApiError> {
        let tags = match self.tag_target(r)? {
            Tagged::Function(n) => self.functions.get(&n).map(|f| f.tags.clone()),
            Tagged::Mapping(u) => self.mappings.get(&u).map(|m| m.tags.clone()),
        };
        Ok(json!({"Tags": tags.unwrap_or_default()}).into())
    }

    pub(crate) fn account_settings(&self, _r: &Req) -> Out {
        let reserved: i64 = self.functions.iter().filter_map(|(_, f)| f.reserved).sum();
        let size: u64 = self.functions.iter().map(|(_, f)| f.latest.code.size).sum();
        json!({
            "AccountLimit": {
                "TotalCodeSize": 80_530_636_800_u64,
                "CodeSizeUnzipped": 262_144_000,
                "CodeSizeZipped": 52_428_800,
                "ConcurrentExecutions": ACCOUNT_CONCURRENCY,
                "UnreservedConcurrentExecutions": ACCOUNT_CONCURRENCY - reserved,
            },
            "AccountUsage": {"TotalCodeSize": size, "FunctionCount": self.functions.len()},
        })
        .into()
    }

    /// A setting accepted and stored as sent: runtime management and recursion.
    pub(crate) fn put_extra(&mut self, r: &Req, key: &str) -> Result<Out, ApiError> {
        let f = r.function()?;
        self.find(r, &f)?;
        let arn = self.arn(r, &f.name, None);
        let stored = Value::Object(r.members().clone()).to_string();
        self.find_mut(r, &f)?.extras.insert(key.to_owned(), stored);
        Ok(if key == "RecursiveLoop" {
            json!({"RecursiveLoop": r.text("RecursiveLoop")}).into()
        } else {
            json!({"UpdateRuntimeOn": r.opt("UpdateRuntimeOn").unwrap_or("Auto"), "FunctionArn": arn,
                "RuntimeVersionArn": r.opt("RuntimeVersionArn")})
            .into()
        })
    }

    pub(crate) fn get_runtime_management(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let stored: Value = function
            .extras
            .get("RuntimeManagementConfig")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null);
        Ok(json!({
            "UpdateRuntimeOn": stored.get("UpdateRuntimeOn").and_then(Value::as_str).unwrap_or("Auto"),
            "FunctionArn": self.arn(r, &f.name, None),
            "RuntimeVersionArn": stored.get("RuntimeVersionArn"),
        })
        .into())
    }

    pub(crate) fn get_recursion(&self, r: &Req) -> Result<Out, ApiError> {
        let f = r.function()?;
        let function = self.find(r, &f)?;
        let stored: Value = function
            .extras
            .get("RecursiveLoop")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(Value::Null);
        Ok(json!({"RecursiveLoop": stored.get("RecursiveLoop").and_then(Value::as_str).unwrap_or("Terminate")}).into())
    }
}

enum Tagged {
    Function(String),
    Mapping(String),
}
