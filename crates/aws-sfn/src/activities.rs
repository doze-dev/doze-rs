//! Activities: the control plane, and `GetActivityTask`, which hands a worker the oldest task waiting for the activity (the long
//! poll is in `lib.rs`, outside the actor, which this answers with an empty task when there is none).

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn;
use crate::core::Core;
use crate::drive::Work;
use crate::model::{Activity, Run};
use crate::req::{Req, invalid_arn, invalid_name, no_activity, put, secs, token, valid_name};

impl Core {
    pub(crate) fn create_activity(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("name");
        if !valid_name(name) {
            return Err(invalid_name(name));
        }
        let arn = arn::activity(r.region(), r.account(), name);
        if let Some(a) = self.activities.get(&arn) {
            return Ok(json!({"activityArn": arn, "creationDate": secs(a.created_ms)}));
        }
        let now = self.now_ms();
        self.activities.insert(
            arn.clone(),
            Activity {
                arn: arn.clone(),
                name: name.to_owned(),
                created_ms: now,
                encryption: r.v.get("encryptionConfiguration").map(Value::to_string),
                tags: r.tags().into_iter().collect::<BTreeMap<_, _>>(),
            },
        );
        Ok(json!({"activityArn": arn, "creationDate": secs(now)}))
    }

    fn activity(&self, given: &str) -> Result<&Activity, ApiError> {
        let p = arn::parts(given).ok_or_else(|| invalid_arn("Arn", given))?;
        if p.kind != "activity" {
            return Err(invalid_arn("Arn", given));
        }
        self.activities.get(given).ok_or_else(|| no_activity(given))
    }

    pub(crate) fn describe_activity(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let a = self.activity(r.s("activityArn"))?;
        let mut out = Map::new();
        out.insert("activityArn".into(), json!(a.arn));
        out.insert("name".into(), json!(a.name));
        out.insert("creationDate".into(), secs(a.created_ms));
        let enc = a
            .encryption
            .as_deref()
            .and_then(|e| serde_json::from_str(e).ok())
            .unwrap_or_else(|| json!({"type": "AWS_OWNED_KEY"}));
        put(&mut out, "encryptionConfiguration", Some(enc));
        Ok(Value::Object(out))
    }

    pub(crate) fn delete_activity(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("activityArn");
        if arn::parts(given).is_none_or(|p| p.kind != "activity") {
            return Err(invalid_arn("Arn", given));
        }
        self.activities.remove(given);
        Ok(json!({}))
    }

    pub(crate) fn list_activities(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut all: Vec<&Activity> = self
            .activities
            .iter()
            .map(|(_, a)| a)
            .filter(|a| {
                arn::parts(&a.arn)
                    .is_some_and(|p| p.region == r.region() && p.account == r.account())
            })
            .collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        let (from, size) = (r.page_from()?, r.page_size(100));
        let page: Vec<Value> = all
            .iter()
            .skip(from)
            .take(size)
            .map(|a| json!({"activityArn": a.arn, "name": a.name, "creationDate": secs(a.created_ms)}))
            .collect();
        let mut out = Map::new();
        out.insert("activities".into(), Value::Array(page));
        if from + size < all.len() {
            out.insert("nextToken".into(), json!(token(from + size)));
        }
        Ok(Value::Object(out))
    }

    /// The oldest task waiting for the activity, marked taken; `{}` when there is none.
    pub(crate) fn take_activity_task(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("activityArn").to_owned();
        self.activity(&given)?;
        let mut oldest: Option<(i64, String)> = None;
        for (k, e) in self.executions.iter() {
            if e.status == Run::Running
                && e.activity.iter().any(|a| a.activity == given && !a.taken)
                && oldest.as_ref().is_none_or(|(t, _)| e.start_ms < *t)
            {
                oldest = Some((e.start_ms, k.to_owned()));
            }
        }
        let Some((_, key)) = oldest else {
            return Ok(json!({}));
        };
        let Some(mut e) = self.executions.get(&key).cloned() else {
            return Ok(json!({}));
        };
        let Some(call) = e
            .activity
            .iter_mut()
            .find(|a| a.activity == given && !a.taken)
        else {
            return Ok(json!({}));
        };
        call.taken = true;
        let (frame, task, input) = (call.frame, call.token.clone(), call.input.clone());
        let env = self.env_of(&e);
        let step = e.exec.activity_started(frame, r.opt("workerName"), &env);
        if let Some(def) = self.definition(&e.definition) {
            self.absorb(&mut e, &def, step);
        }
        self.store(e);
        self.queue.push_back(Work::Run(key));
        self.process();
        self.effects.taken = true;
        Ok(json!({"taskToken": task, "input": input}))
    }
}
