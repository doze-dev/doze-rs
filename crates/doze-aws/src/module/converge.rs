//! Converge: the declared resources (`template.rs`) kept as one CloudFormation stack, `doze`, on the instance's own
//! CloudFormation: created on the first converge, then updated through a change set, so only what changed is touched
//! and an unchanged doze.hcl changes nothing. A failure names the resources that failed and why, from the stack's
//! events; CloudFormation has rolled the stack back by then, so a converge never leaves half of a change made.

use std::time::{Duration, Instant};

use doze_module::caps::{Converge, Objects};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Object, Toolchain};
use doze_module::{Cx, Result, help};

use super::spec::AwsSpec;
use super::{Aws, REGION, backend};
use crate::cfn::{Cfn, CfnError};
use crate::client::Client;

/// The stack the module keeps.
pub(crate) const STACK: &str = "doze";
/// How long one create or update may take.
const SETTLE: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(250);

fn failed(e: &CfnError) -> Error {
    let kind = match e {
        CfnError::Api { .. } => ErrorKind::Invalid,
        CfnError::Wire(_) => ErrorKind::Unavailable,
    };
    Error::new(kind, format!("keeping the aws stack: {e}"))
}

fn cfn_of(addr: &str) -> Result<Cfn> {
    let client = Client::new(&format!("http://{addr}"), REGION, "test", "test")
        .map_err(|e| Error::new(ErrorKind::Internal, e))?;
    Ok(Cfn::new(client))
}

impl Converge for Aws {
    async fn converge(&self, cx: &Cx<AwsSpec>, _tc: &Toolchain) -> Result<()> {
        let addr = backend(&cx.instance)?;
        let body = super::template::template(&cx.spec, &cx.instance.dir).to_string();
        let empty = super::declared_nothing(&cx.spec);
        let data = std::path::PathBuf::from(&cx.instance.data_dir);
        let run = move || -> std::result::Result<_, Error> {
            let cfn = cfn_of(&addr)?;
            apply(&cfn, &body, empty)?;
            // After the stack, so what doze.hcl declares is the stack's; then what the Go module left (godata.rs).
            let client = Client::new(&format!("http://{addr}"), REGION, "test", "test")
                .map_err(|e| Error::new(ErrorKind::Internal, e))?;
            super::godata::replay(&data, &client).map_err(|why| {
                Error::new(
                    ErrorKind::Unavailable,
                    format!(
                        "carrying over the Go doze-aws data (kept in {}/; the next start goes on): {why}",
                        super::godata::KEPT
                    ),
                )
            })
        };
        let replayed = tokio::task::spawn_blocking(run)
            .await
            .map_err(|e| Error::new(ErrorKind::Internal, format!("converge: {e}")))??;
        if let Some(r) = replayed {
            cx.log(
                2,
                format!(
                    "took over the Go doze-aws data: {} buckets, {} objects, {} queues, {} messages (Go's files kept in {}/)",
                    r.buckets,
                    r.objects,
                    r.queues,
                    r.messages,
                    super::godata::KEPT
                ),
            );
            for what in r.left {
                cx.log(1, format!("not carried from the Go data: {what}"));
            }
        }
        Ok(())
    }
}

/// Creates or updates the stack to `body`. With nothing declared and no stack yet, there is nothing to do.
fn apply(cfn: &Cfn, body: &str, empty: bool) -> Result<()> {
    let mut stack = cfn.stack(STACK).map_err(|e| failed(&e))?;
    if let Some(s) = &stack
        && s.status.ends_with("_IN_PROGRESS")
    {
        settle(cfn)?;
        stack = cfn.stack(STACK).map_err(|e| failed(&e))?;
    }
    let there = match &stack {
        Some(s) => unstick(cfn, &s.status)?,
        None => false,
    };
    match (there, empty) {
        (false, true) => Ok(()),
        (false, false) => {
            let seen = seen(cfn);
            cfn.create_stack_clean(STACK, body)
                .map_err(|e| failed(&e))?;
            outcome(cfn, &settle(cfn)?, "CREATE_COMPLETE", &seen)
        }
        (true, _) => update(cfn, body),
    }
}

fn update(cfn: &Cfn, body: &str) -> Result<()> {
    // A name of its own per converge: a stack keeps its executed change sets' names.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let set = format!("doze-{now}");
    let id = cfn
        .create_change_set(STACK, &set, body, &[])
        .map_err(|e| failed(&e))?;
    let started = Instant::now();
    let changes = loop {
        let c = cfn.change_set(&id).map_err(|e| failed(&e))?;
        if !matches!(c.status.as_str(), "CREATE_PENDING" | "CREATE_IN_PROGRESS") {
            break c;
        }
        if started.elapsed() > SETTLE {
            let _ = cfn.delete_change_set(&id);
            return Err(Error::new(
                ErrorKind::TimedOut,
                "the change set never settled",
            ));
        }
        std::thread::sleep(POLL);
    };
    if changes.status == "FAILED" {
        let _ = cfn.delete_change_set(&id);
        if changes.reason.contains("didn't contain changes")
            || changes.reason.contains("No updates")
        {
            return Ok(());
        }
        return Err(Error::new(
            ErrorKind::Invalid,
            format!("keeping the aws stack: {}", changes.reason),
        ));
    }
    let seen = seen(cfn);
    cfn.execute_change_set(&id).map_err(|e| failed(&e))?;
    outcome(cfn, &settle(cfn)?, "UPDATE_COMPLETE", &seen)
}

/// Moves on a stack a converge cannot update (a first create that failed, an update or delete a stop cut short,
/// which doze-aws marks failed when it starts again); whether the stack is still there to update.
fn unstick(cfn: &Cfn, status: &str) -> Result<bool> {
    let call = |action: &str| cfn.stack_call(action, STACK).map_err(|e| failed(&e));
    match status {
        // Nothing to keep: a create that never finished, or a stack half deleted. AWS makes you delete it.
        "ROLLBACK_COMPLETE" | "ROLLBACK_FAILED" | "REVIEW_IN_PROGRESS" | "CREATE_FAILED"
        | "DELETE_FAILED" => {
            cfn.delete_stack(STACK).map_err(|e| failed(&e))?;
            match settle(cfn)?.as_str() {
                "" | "DELETE_COMPLETE" => Ok(false),
                other => Err(Error::new(
                    ErrorKind::Invalid,
                    format!("the aws stack could not be cleared: it is {other}"),
                )),
            }
        }
        "UPDATE_FAILED" => {
            call("RollbackStack")?;
            settle(cfn).map(|_| true)
        }
        "UPDATE_ROLLBACK_FAILED" => {
            call("ContinueUpdateRollback")?;
            settle(cfn).map(|_| true)
        }
        _ => Ok(true),
    }
}

/// The stack's events so far, so a failure reports only what this converge did.
fn seen(cfn: &Cfn) -> Vec<String> {
    cfn.events(STACK)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.id)
        .collect()
}

/// Waits until the stack is no longer changing; its status then ("" when it is gone).
fn settle(cfn: &Cfn) -> Result<String> {
    let started = Instant::now();
    loop {
        let status = cfn
            .stack(STACK)
            .map_err(|e| failed(&e))?
            .map(|s| s.status)
            .unwrap_or_default();
        if !status.ends_with("_IN_PROGRESS") || status == "REVIEW_IN_PROGRESS" {
            return Ok(status);
        }
        if started.elapsed() > SETTLE {
            return Err(Error::new(
                ErrorKind::TimedOut,
                format!(
                    "the aws stack is still {status} after {}s",
                    SETTLE.as_secs()
                ),
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// `Ok` when the stack reached `want`; otherwise what failed, from its events.
fn outcome(cfn: &Cfn, status: &str, want: &str, seen: &[String]) -> Result<()> {
    if status == want {
        return Ok(());
    }
    let events = cfn.events(STACK).unwrap_or_default();
    let why: Vec<String> = events
        .iter()
        .filter(|e| {
            e.status.ends_with("_FAILED")
                && !e.reason.is_empty()
                && e.logical != STACK
                && !seen.contains(&e.id)
        })
        .take(5)
        .map(|e| format!("{} ({}): {}", e.logical, e.kind, e.reason))
        .collect();
    let detail = if why.is_empty() {
        String::new()
    } else {
        format!(": {}", why.join("; "))
    };
    Err(Error::new(
        ErrorKind::Invalid,
        format!("the aws stack ended {status}, not {want}{detail}"),
    ))
}

impl Objects for Aws {
    /// Each declared resource, as the stack names it (its logical id) with its declaration hashed, so `doze plan` shows
    /// what a converge would create or change.
    fn objects(&self, cx: &Cx<AwsSpec>) -> Result<Vec<Object>> {
        let t = super::template::template(&cx.spec, &cx.instance.dir);
        let resources = t
            .get("Resources")
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default();
        Ok(resources
            .iter()
            .filter(|(id, _)| id.as_str() != "DozeKept")
            .map(|(id, r)| Object {
                kind: r
                    .get("Type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                name: id.clone(),
                hash: help::object_hash(&r.to_string()),
            })
            .collect())
    }
}
