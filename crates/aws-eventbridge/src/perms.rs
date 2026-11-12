//! The resource policy of a bus: `PutPermission` adds a statement (or replaces the whole policy), `RemovePermission` takes one away.
//! The policy is returned by `DescribeEventBus` and is what a cross-account `PutEvents` is judged by. The statement a `Principal`
//! account id and an `Action` make is the one AWS documents; the wording of the refusals is from memory.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::key;
use crate::req::{Req, invalid, not_found};

impl Core {
    pub(crate) fn put_permission(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.bus(r.account(), r.opt("EventBusName"))?.name.clone();
        let policy = match r.opt("Policy") {
            Some(text) => {
                serde_json::from_str::<Value>(text)
                    .map_err(|_| invalid("Parameter Policy is not valid."))?;
                text.to_owned()
            }
            None => {
                let bus = self.bus(r.account(), Some(&name))?;
                let statement = statement(r, &bus.arn)?;
                let mut doc = bus
                    .policy
                    .as_deref()
                    .and_then(|p| serde_json::from_str::<Value>(p).ok())
                    .unwrap_or_else(|| json!({"Version": "2012-10-17", "Statement": []}));
                let list = doc.get_mut("Statement").and_then(Value::as_array_mut);
                let sid = statement.get("Sid").cloned();
                if list
                    .as_ref()
                    .is_some_and(|l| l.iter().any(|s| s.get("Sid") == sid.as_ref()))
                {
                    return Err(invalid("Statement already exists."));
                }
                if let Some(list) = list {
                    list.push(statement);
                }
                doc.to_string()
            }
        };
        let now = self.now_ms();
        if let Some(b) = self.buses.get_mut(&key(r.account(), &[&name])) {
            b.policy = Some(policy);
            b.modified_ms = now;
        }
        Ok(Value::Null)
    }

    pub(crate) fn remove_permission(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.bus(r.account(), r.opt("EventBusName"))?.name.clone();
        let now = self.now_ms();
        let Some(b) = self.buses.get_mut(&key(r.account(), &[&name])) else {
            return Ok(Value::Null);
        };
        if r.flag("RemoveAllPermissions") {
            b.policy = None;
            b.modified_ms = now;
            return Ok(Value::Null);
        }
        let sid = r.s("StatementId");
        let mut doc = b
            .policy
            .as_deref()
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .ok_or_else(missing)?;
        let list = doc
            .get_mut("Statement")
            .and_then(Value::as_array_mut)
            .ok_or_else(missing)?;
        let before = list.len();
        list.retain(|s| s.get("Sid").and_then(Value::as_str) != Some(sid));
        if list.len() == before {
            return Err(missing());
        }
        b.policy = (!list.is_empty()).then(|| doc.to_string());
        b.modified_ms = now;
        Ok(Value::Null)
    }
}

fn missing() -> ApiError {
    not_found("Statement with the provided id does not exist.")
}

/// The statement for `StatementId`, `Principal`, `Action` and `Condition` on the bus.
fn statement(r: &Req<'_>, bus_arn: &str) -> Result<Value, ApiError> {
    let need = |m: &str| {
        r.opt(m)
            .ok_or_else(|| invalid(format!("Parameter {m} is required.")))
    };
    let (sid, principal, action) = (need("StatementId")?, need("Principal")?, need("Action")?);
    let who = if principal == "*" {
        json!("*")
    } else {
        json!({"AWS": format!("arn:aws:iam::{principal}:root")})
    };
    let mut s = serde_json::Map::new();
    s.insert("Sid".into(), json!(sid));
    s.insert("Effect".into(), json!("Allow"));
    s.insert("Principal".into(), who);
    s.insert("Action".into(), json!(action));
    s.insert("Resource".into(), json!(bus_arn));
    if let Some(c) = r.v.get("Condition") {
        let get = |m: &str| c.get(m).and_then(Value::as_str).unwrap_or_default();
        s.insert(
            "Condition".into(),
            json!({ get("Type"): { get("Key"): get("Value") } }),
        );
    }
    Ok(Value::Object(s))
}
