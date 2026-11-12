//! One resource made, updated or replaced: its properties' dynamic references resolved, the call made through its service (or, for
//! a skipped type, an identity given), and the result handed back to the loop.

use std::sync::Arc;

use serde_json::Value;

use super::Runner;
use crate::model::Res;
use crate::res::policy::Class;
use crate::res::{self, Made, Rc};

/// What is done to a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Create,
    Update,
    Replace,
}

/// A resource about to be acted on.
#[derive(Debug, Clone)]
pub(super) struct Launch {
    pub(super) logical: String,
    pub(super) ty: String,
    pub(super) step: Step,
    /// The properties, resolved but for dynamic references (they are resolved at the call and never kept).
    pub(super) props: Value,
    pub(super) old: Option<Res>,
    pub(super) class: Class,
}

/// Types that wait for eventual consistency after they are configured (inferred from the recording of 2026-11-12: a stack of SQS
/// queues alone shows the stack-level `Eventual consistency check initiated` event; a stack with an SNS topic and an SSM parameter
/// beside its queue does not).
pub(super) fn eventually_consistent(ty: &str) -> bool {
    ty == "AWS::SQS::Queue"
}

/// Performs the launch.
pub(super) async fn perform(rc: Rc, l: Launch) -> Result<(Made, Option<String>), String> {
    if let Class::Skip(why) = &l.class {
        let physical = match (&l.old, l.step) {
            (Some(old), Step::Update) => old.physical.clone().unwrap_or_default(),
            _ => res::skip::physical(&rc, &l.props),
        };
        return Ok((Made::new(physical), Some(why.clone())));
    }
    let mut props = l.props.clone();
    crate::dynref::resolve(&mut props, rc.peers.as_deref(), &rc.caller).await?;
    let made = match (l.step, &l.old) {
        (Step::Update, Some(old)) => res::update(&rc, old, &props).await?,
        _ => res::create(&rc, &props).await?,
    };
    Ok((made, None))
}

/// The context a resource of `ty` is made in.
pub(super) fn context(runner: &Arc<Runner>, job: &super::Job, logical: &str, ty: &str) -> Rc {
    Rc {
        peers: runner.peers.clone(),
        caller: job.caller.clone(),
        stack_name: job.stack_name.clone(),
        stack_id: job.stack_id.clone(),
        logical: logical.to_owned(),
        ty: ty.to_owned(),
        tags: job.tags.clone(),
        logical_names: runner.config.logical_names,
        runner: Arc::clone(runner),
        token: aws_seal::uuid_v4().unwrap_or_default(),
    }
}

/// Whether a change from `old` to `new` replaces the resource.
pub(super) fn replaces(ty: &str, old: &Value, new: &Value) -> bool {
    res::create_only(ty)
        .iter()
        .any(|k| old.get(*k) != new.get(*k))
}
