//! The forward pass of a create or update (and of an update's rollback): every live resource in dependency order, each made,
//! updated, replaced or left alone; independent resources at the same time; nothing new started once one has failed.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use tokio::task::JoinSet;

use super::launch::{self, Launch, Step};
use super::run::Op;
use crate::model::{Draft, Progress, Res};
use crate::res::policy::{self, Class};
use crate::res::{self, Made};
use crate::tmpl::eval::Fault;

/// How many resources are worked on at once.
const AT_ONCE: usize = 16;

/// What the plan for one resource is.
enum Plan {
    Nothing,
    Fail(String),
    Go(Box<Launch>),
}

type Result3 = (Launch, Result<(Made, Option<String>), String>);

impl Op {
    /// Runs the forward pass. `true` when everything it touched succeeded.
    pub(super) async fn forward(&mut self) -> bool {
        let mut waiting: BTreeMap<String, BTreeSet<String>> = self.w.graph();
        let mut done: BTreeSet<String> = BTreeSet::new();
        let mut running: JoinSet<Result3> = JoinSet::new();
        let mut held: Vec<Draft> = Vec::new();
        let mut stop = false;
        loop {
            let mut moved = true;
            while moved && !stop {
                moved = false;
                let ready: Vec<String> = waiting
                    .iter()
                    .filter(|(_, w)| w.iter().all(|d| done.contains(d)))
                    .map(|(k, _)| k.clone())
                    .collect();
                for name in ready {
                    if running.len() >= AT_ONCE {
                        break;
                    }
                    waiting.remove(&name);
                    match self.plan(&name) {
                        Plan::Nothing => {
                            done.insert(name);
                            moved = true;
                        }
                        Plan::Fail(why) => {
                            self.fail_early(&name, why).await;
                            stop = true;
                        }
                        Plan::Go(l) => self.launch(&mut running, *l).await,
                    }
                }
            }
            let Some(joined) = running.join_next().await else {
                break;
            };
            let Ok((l, result)) = joined else {
                stop = true;
                continue;
            };
            let name = l.logical.clone();
            match result {
                Ok((made, note)) => {
                    self.succeeded(&l, made, note, &mut held).await;
                    done.insert(name);
                }
                Err(why) => {
                    self.failed_with(&l, why).await;
                    stop = true;
                }
            }
        }
        self.release(held, stop).await;
        !stop && self.failed.is_empty()
    }

    /// Reports the completions held back for eventual consistency. When every resource made was waiting for it, the stack says so
    /// before they complete.
    async fn release(&mut self, mut held: Vec<Draft>, stop: bool) {
        if held.is_empty() {
            return;
        }
        let all = self
            .created
            .iter()
            .all(|n| launch::eventually_consistent(&self.w.type_of(n)));
        if !stop && all && !self.rolling_back && self.job.kind == super::Kind::Create {
            let mut d = Draft::stack(
                &self.job.stack_name,
                &self.job.stack_id,
                "CREATE_IN_PROGRESS",
                Some("Eventual consistency check initiated"),
            );
            d.detailed = Some("CONFIGURATION_COMPLETE".into());
            held.insert(0, d);
        }
        self.runner.events(&self.job.stack_id, held).await;
    }

    /// What to do with one resource now that what it waits for is done.
    fn plan(&self, name: &str) -> Plan {
        let ty = self.w.type_of(name);
        let class = policy::class(&ty, &self.runner.config);
        let props = match self.w.props(name) {
            Ok(p) => p,
            Err(Fault::Error(m)) => return Plan::Fail(m),
            // (memory)
            Err(Fault::Pending(x)) => {
                return Plan::Fail(format!("Template error: resource {x} has no value yet"));
            }
        };
        let current = self
            .w
            .resources
            .get(name)
            .filter(|r| r.physical.is_some() && !r.status.starts_with("DELETE"));
        let Some(old) = current else {
            return Plan::Go(Box::new(Launch {
                logical: name.to_owned(),
                ty,
                step: Step::Create,
                props,
                old: None,
                class,
            }));
        };
        let before = old.props_value();
        let taggable = props.get("Tags").is_some() || before.get("Tags").is_some();
        if before == props
            && !(self.job.tags_changed && taggable)
            && !self.job.retain.contains(name)
        {
            return Plan::Nothing;
        }
        if self.job.retain.contains(name) {
            // `ContinueUpdateRollback` was told to skip it.
            return Plan::Nothing;
        }
        let step = if matches!(class, Class::Map) && launch::replaces(&ty, &before, &props) {
            if let Some(k) = res::name_property(&ty)
                && let Some(n) = props.get(k).filter(|n| before.get(k) == Some(*n))
            {
                // (memory)
                return Plan::Fail(format!(
                    "CloudFormation cannot update a stack when a custom-named resource requires replacing. Rename {} and update the stack again.",
                    n.as_str().map_or_else(|| n.to_string(), str::to_owned)
                ));
            }
            Step::Replace
        } else {
            Step::Update
        };
        if let Some(policy) = self.job.policy.as_deref().filter(|_| !self.rolling_back) {
            let action = if step == Step::Replace {
                "Update:Replace"
            } else {
                "Update:Modify"
            };
            if let Err(why) = crate::stackpolicy::check(policy, action, name, &ty) {
                return Plan::Fail(why);
            }
        }
        Plan::Go(Box::new(Launch {
            logical: name.to_owned(),
            ty,
            step,
            props,
            old: Some(old.clone()),
            class,
        }))
    }

    fn verb(&self, l: &Launch) -> &'static str {
        if l.step == Step::Create {
            "CREATE"
        } else {
            "UPDATE"
        }
    }

    async fn launch(&mut self, running: &mut JoinSet<Result3>, l: Launch) {
        let verb = self.verb(&l);
        let physical = l
            .old
            .as_ref()
            .and_then(|o| o.physical.clone())
            .unwrap_or_default();
        let reason = (l.step == Step::Replace)
            .then(|| "Requested update requires the creation of a new physical resource; hence creating one.".to_owned());
        let draft = Draft {
            logical: l.logical.clone(),
            physical: physical.clone(),
            ty: l.ty.clone(),
            status: format!("{verb}_IN_PROGRESS"),
            reason,
            props: Some(l.props.to_string()),
            detailed: None,
        };
        let mut res = l.old.clone().unwrap_or_default();
        res.ty = l.ty.clone();
        res.status = format!("{verb}_IN_PROGRESS");
        res.reason = None;
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events: vec![draft],
                resources: vec![(l.logical.clone(), Some(res))],
                ..Progress::default()
            })
            .await;
        let rc = launch::context(&self.runner, &self.job, &l.logical, &l.ty);
        running.spawn(async move {
            let r = launch::perform(rc, l.clone()).await;
            (l, r)
        });
    }

    async fn succeeded(
        &mut self,
        l: &Launch,
        made: Made,
        note: Option<String>,
        held: &mut Vec<Draft>,
    ) {
        let verb = self.verb(l);
        let res = Res {
            ty: l.ty.clone(),
            physical: Some(made.physical.clone()),
            status: format!("{verb}_COMPLETE"),
            reason: note.clone(),
            updated_ms: self.runner.clock.now_ms(),
            props: l.props.to_string(),
            attrs: Value::Object(made.attrs.clone()).to_string(),
            metadata: self
                .w
                .declared(&l.logical)
                .and_then(|r| r.get("Metadata"))
                .map(Value::to_string),
            description: None,
            skipped: matches!(l.class, Class::Skip(_)),
        };
        match l.step {
            Step::Create => {
                self.created.insert(l.logical.clone());
            }
            Step::Replace => {
                if let Some(old) = &l.old {
                    self.replaced.insert(l.logical.clone(), old.clone());
                }
            }
            Step::Update => {}
        }
        self.w.resources.insert(l.logical.clone(), res.clone());
        let event = |status: String, reason: Option<String>, detailed: Option<&str>| Draft {
            logical: l.logical.clone(),
            physical: made.physical.clone(),
            ty: l.ty.clone(),
            status,
            reason,
            props: Some(l.props.to_string()),
            detailed: detailed.map(str::to_owned),
        };
        let mut events = Vec::new();
        if l.step != Step::Update {
            events.push(event(
                format!("{verb}_IN_PROGRESS"),
                Some("Resource creation Initiated".into()),
                None,
            ));
        }
        let complete = event(format!("{verb}_COMPLETE"), note, None);
        let mut shown = res.clone();
        if l.step == Step::Create && launch::eventually_consistent(&l.ty) && !self.rolling_back {
            events.push(event(
                format!("{verb}_IN_PROGRESS"),
                Some("Eventual consistency check initiated".into()),
                Some("CONFIGURATION_COMPLETE"),
            ));
            held.push(complete);
            shown.status = "CREATE_IN_PROGRESS".into();
        } else {
            events.push(complete);
        }
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events,
                resources: vec![(l.logical.clone(), Some(shown))],
                ..Progress::default()
            })
            .await;
    }

    async fn failed_with(&mut self, l: &Launch, why: String) {
        let verb = self.verb(l);
        self.failed
            .push((l.logical.clone(), l.step == Step::Create));
        let mut res = l.old.clone().unwrap_or_default();
        res.ty = l.ty.clone();
        res.status = format!("{verb}_FAILED");
        res.reason = Some(why.clone());
        res.updated_ms = self.runner.clock.now_ms();
        if l.step == Step::Update && !l.props.is_null() {
            // What was attempted is what a rollback puts back from.
            res.props = l.props.to_string();
        }
        if l.step == Step::Create {
            res.props = l.props.to_string();
            res.physical = None;
        }
        self.w.resources.insert(l.logical.clone(), res.clone());
        let draft = Draft {
            logical: l.logical.clone(),
            physical: res.physical.clone().unwrap_or_default(),
            ty: l.ty.clone(),
            status: format!("{verb}_FAILED"),
            reason: Some(why),
            props: Some(l.props.to_string()),
            detailed: None,
        };
        self.runner
            .report(Progress {
                stack_id: self.job.stack_id.clone(),
                events: vec![draft],
                resources: vec![(l.logical.clone(), Some(res))],
                ..Progress::default()
            })
            .await;
    }

    /// A resource that fails before it is started (its properties do not resolve, or it cannot be replaced).
    async fn fail_early(&mut self, name: &str, why: String) {
        let ty = self.w.type_of(name);
        let old = self.w.resources.get(name).cloned();
        let step = if old.as_ref().is_some_and(|o| o.physical.is_some()) {
            Step::Update
        } else {
            Step::Create
        };
        let l = Launch {
            logical: name.to_owned(),
            ty,
            step,
            props: Value::Null,
            old,
            class: Class::Map,
        };
        let verb = self.verb(&l);
        self.runner
            .events(
                &self.job.stack_id,
                vec![Draft {
                    logical: name.to_owned(),
                    physical: String::new(),
                    ty: l.ty.clone(),
                    status: format!("{verb}_IN_PROGRESS"),
                    reason: None,
                    props: None,
                    detailed: None,
                }],
            )
            .await;
        self.failed_with(&l, why).await;
    }
}
