//! Nested stacks (`AWS::CloudFormation::Stack`): the parent's job asks the actor to create, update or delete a child stack through the
//! same code as the public operations (the child is a real stack, with `ParentId` and `RootId`, its own events and its own rollback),
//! and waits for the child's end. A child's outputs are the parent resource's `Outputs.<Key>` attributes; its `Ref` is the child's id.

use serde_json::{Value, json};
use tokio::sync::oneshot;

use aws_schema::arena::{Arena, NIL};
use aws_wire::RequestId;
use aws_wire::service::{Call, Caller};

use crate::core::{Core, Request};
use crate::engine::Outcome;
use crate::j::J;
use crate::model::Res;
use crate::req::Req;
use crate::res::{Done, Made, Rc, text};

/// What a parent's job asks for.
#[derive(Debug)]
pub(crate) struct Ask {
    pub(crate) caller: Caller,
    pub(crate) parent_id: String,
    /// `CreateStack`, `UpdateStack` or `DeleteStack`.
    pub(crate) op: &'static str,
    pub(crate) body: Value,
    pub(crate) done: oneshot::Sender<Outcome>,
}

impl Core {
    /// Runs an operation on a child stack for its parent; the child's id, or why it could not start.
    pub(crate) async fn child(&mut self, ask: Ask) -> Result<String, String> {
        let Ask {
            mut caller,
            parent_id,
            op,
            body,
            done,
        } = ask;
        caller.service = Some("cloudformation".into());
        let index = aws_model_cloudformation::tables::TABLES
            .find_op(op)
            .unwrap_or(0);
        let call = Call {
            op: index,
            id: RequestId::new(),
            caller,
            body: body.to_string(),
            arena: Arena::new(),
            root: NIL,
        };
        let r = Req::new(&call);
        let jobs_before = self.started.len();
        let answer = match op {
            "CreateStack" => self.create_stack(&r).await,
            "UpdateStack" => self.update_stack(&r).await,
            _ => self.delete_stack(&r),
        };
        let name = r.s("StackName").to_owned();
        let id = self.find(&name).map(|s| s.id.clone()).unwrap_or(name);
        match answer {
            Ok(_) if self.started.len() > jobs_before => {
                let root = self
                    .stacks
                    .get(&parent_id)
                    .map(|p| p.root_id.clone().unwrap_or_else(|| p.id.clone()));
                if let Some(s) = self.stacks.get_mut(&id) {
                    s.parent_id = Some(parent_id);
                    s.root_id = root;
                }
                if let Some(job) = self.started.last_mut() {
                    job.done = Some(done);
                }
                Ok(id)
            }
            // Nothing to do (a delete of a stack already gone).
            Ok(_) => {
                drop(done.send(Outcome {
                    status: "DELETE_COMPLETE".into(),
                    ..Outcome::default()
                }));
                Ok(id)
            }
            Err(e) if e.message == "No updates are to be performed." => {
                let outputs = self
                    .stacks
                    .get(&id)
                    .map(|s| s.outputs.clone())
                    .unwrap_or_default();
                drop(done.send(Outcome {
                    status: "UPDATE_COMPLETE".into(),
                    reason: None,
                    outputs,
                }));
                Ok(id)
            }
            Err(e) => Err(e.message),
        }
    }
}

/// The parameters of a nested stack, as `[{ParameterKey, ParameterValue}]`.
fn parameters(p: &Value) -> Value {
    let list: Vec<Value> = p
        .get("Parameters")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let text = match v {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a
                            .iter()
                            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))
                            .collect::<Vec<_>>()
                            .join(","),
                        other => other.to_string(),
                    };
                    json!({"ParameterKey": k, "ParameterValue": text})
                })
                .collect()
        })
        .unwrap_or_default();
    Value::Array(list)
}

/// The body of a child's create or update.
fn body(name: &str, p: &Value, rc: &Rc) -> Value {
    let mut b = json!({"StackName": name, "TemplateURL": text(p, "TemplateURL"), "Parameters": parameters(p),
        "Capabilities": ["CAPABILITY_IAM", "CAPABILITY_NAMED_IAM", "CAPABILITY_AUTO_EXPAND"]});
    let mut tags: Vec<Value> = rc
        .tags
        .iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect();
    if let Some(Value::Array(own)) = p.get("Tags") {
        tags.extend(own.iter().cloned());
    }
    b.put("Tags", Value::Array(tags));
    for k in ["NotificationARNs", "TimeoutInMinutes"] {
        if let Some(v) = p.get(k) {
            b.put(k, v.clone());
        }
    }
    b
}

/// Asks the actor and waits for the child's end.
async fn run(rc: &Rc, op: &'static str, body: Value) -> Result<(String, Outcome), String> {
    let (tx, rx) = oneshot::channel();
    let ask = Ask {
        caller: rc.caller.clone(),
        parent_id: rc.stack_id.clone(),
        op,
        body,
        done: tx,
    };
    let answer = rc.runner.ask(Request::Child(Box::new(ask))).await?;
    let id = answer
        .child
        .unwrap_or_else(|| Err("the child stack could not be started".into()))?;
    let outcome = rx
        .await
        .map_err(|_| "the child stack's operation was cut off".to_owned())?;
    Ok((id, outcome))
}

fn made(id: &str, outcome: &Outcome) -> Made {
    let mut m = Made::new(id);
    for o in &outcome.outputs {
        m.attrs
            .insert(format!("Outputs.{}", o.key), Value::String(o.value.clone()));
    }
    m
}

pub(crate) async fn create(rc: &Rc, p: &Value) -> Done {
    let name = crate::res::generated_name(rc, 128, false);
    let (id, outcome) = run(rc, "CreateStack", body(&name, p, rc))
        .await
        .map_err(|e| rc.handler(&e, "InvalidRequest"))?;
    if outcome.status != "CREATE_COMPLETE" {
        // (memory)
        return Err(format!(
            "Embedded stack {id} was not successfully created: {}",
            outcome.reason.unwrap_or_default()
        ));
    }
    Ok(made(&id, &outcome))
}

pub(crate) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    let (_, outcome) = run(rc, "UpdateStack", body(&id, p, rc))
        .await
        .map_err(|e| rc.handler(&e, "InvalidRequest"))?;
    if outcome.status != "UPDATE_COMPLETE" {
        // (memory)
        return Err(format!(
            "Embedded stack {id} was not successfully updated. Currently in {}.",
            outcome.status
        ));
    }
    Ok(made(&id, &outcome))
}

pub(crate) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let (_, outcome) = run(rc, "DeleteStack", json!({"StackName": id})).await?;
    if outcome.status == "DELETE_COMPLETE" {
        Ok(())
    } else {
        // (memory)
        Err(format!(
            "Embedded stack {id} was not successfully deleted: {}",
            outcome.reason.unwrap_or_default()
        ))
    }
}
