//! Versions: a published version freezes the definition and the role; `revisionId` guards a publish; a version an alias routes to
//! cannot be deleted.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::arn::{self, Target};
use crate::core::Core;
use crate::machines::publish;
use crate::req::{Req, conflict, invalid_arn, no_machine, secs, token};

impl Core {
    pub(crate) fn publish_version(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let now = self.now_ms();
        let m = self
            .machines
            .get_mut(given)
            .ok_or_else(|| no_machine(given))?;
        if let Some(rev) = r.opt("revisionId")
            && rev != m.revision
        {
            // (memory)
            return Err(conflict(format!(
                "Failed to publish the State Machine version for revision {rev}. The current State Machine revision is {}.",
                m.revision
            )));
        }
        let v = publish(m, r.opt("description"), now);
        let created = m.versions.last().map_or(now, |x| x.created_ms);
        Ok(json!({"stateMachineVersionArn": v, "creationDate": secs(created)}))
    }

    pub(crate) fn delete_version(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineVersionArn");
        let Some((p, Target::Version(name, n))) = arn::target(given) else {
            return Err(invalid_arn("Arn", given));
        };
        let key = arn::machine(p.region, p.account, &name);
        let Some(m) = self.machines.get_mut(&key) else {
            return Ok(json!({}));
        };
        let routed: Vec<String> = m
            .aliases
            .iter()
            .filter(|a| a.routes.iter().any(|(v, _)| *v == n))
            .map(|a| format!("{key}:{}", a.name))
            .collect();
        if !routed.is_empty() {
            // (memory)
            return Err(conflict(format!(
                "Version to be deleted must not be referenced by an alias. Current list of aliases referencing this version: [{}]",
                routed.join(", ")
            )));
        }
        m.versions.retain(|v| v.number != n);
        Ok(json!({}))
    }

    pub(crate) fn list_versions(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let m = self.machines.get(given).ok_or_else(|| no_machine(given))?;
        let all: Vec<Value> = m
            .versions
            .iter()
            .rev()
            .map(|v| json!({"stateMachineVersionArn": format!("{}:{}", m.arn, v.number), "creationDate": secs(v.created_ms)}))
            .collect();
        let (from, size) = (r.page_from()?, r.page_size(100));
        let mut out = json!({"stateMachineVersions": all.iter().skip(from).take(size).cloned().collect::<Vec<_>>()});
        if from + size < all.len() {
            out["nextToken"] = json!(token(from + size));
        }
        Ok(out)
    }
}
