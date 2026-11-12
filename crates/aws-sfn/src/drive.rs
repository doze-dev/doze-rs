//! Running executions inside the actor: a queue of work (run what can run, deliver an answer, wake what is due), each step's events
//! kept as history, its task calls handed out as jobs (or done here, for activities and child executions, which need no I/O), and
//! its end recorded. Nothing here waits.

use std::sync::Arc;

use aws_asl::{Definition, Dispatch, End, Env, Outcome, Step};
use aws_wire::service::Caller;
use serde_json::json;

use crate::core::Core;
use crate::effects::Job;
use crate::model::{ActivityCall, Execution, Run};

/// How many pieces of work one call may do (a chain of child executions that end at once could otherwise grow without bound).
const MOST_WORK: usize = 10_000;
/// How many events one execution may have (AWS's quota is 25,000).
pub(crate) const MOST_EVENTS: u64 = 25_000;

/// One piece of work on an execution.
#[derive(Debug)]
pub(crate) enum Work {
    Run(String),
    Deliver {
        execution: String,
        frame: u32,
        seq: u32,
        outcome: Outcome,
    },
    Wake(String),
}

impl Core {
    /// The environment of one step of `e`.
    pub(crate) fn env_of(&self, e: &Execution) -> Env {
        let mut seed = [0u8; 8];
        // WHY: without the system's randomness the clock is a fair seed for jitter and identifiers (tokens are checked against
        // the execution that made them, not trusted for being unguessable).
        if getrandom::fill(&mut seed).is_err() {
            seed = self.now_ms().to_le_bytes();
        }
        let mut env = Env::new(self.now_ms(), u64::from_le_bytes(seed));
        env.tokens = !e.express;
        env.wait_scale = self.config.wait_scale;
        if let Some(p) = crate::arn::parts(&e.arn) {
            p.region.clone_into(&mut env.region);
            p.account.clone_into(&mut env.account);
        }
        env
    }

    /// Does the queued work.
    pub(crate) fn process(&mut self) {
        let mut budget = MOST_WORK;
        while let Some(w) = self.queue.pop_front() {
            if budget == 0 {
                tracing::warn!(
                    "step functions: too much work in one call; the rest waits for the next"
                );
                self.queue.clear();
                return;
            }
            budget -= 1;
            self.work(w);
        }
    }

    fn work(&mut self, w: Work) {
        let arn = match &w {
            Work::Run(a) | Work::Wake(a) | Work::Deliver { execution: a, .. } => a.clone(),
        };
        let Some(mut e) = self.executions.get(&arn).cloned() else {
            return;
        };
        if e.status != Run::Running {
            return;
        }
        let Some(def) = self.definition(&e.definition) else {
            self.end(
                &mut e,
                End::Failed(aws_asl::Failure::runtime(
                    "The definition could not be read",
                )),
            );
            self.store(e);
            return;
        };
        let env = self.env_of(&e);
        let step = match w {
            Work::Run(_) => e.exec.run(&def, &env),
            Work::Wake(_) => e.exec.wake(&def, &env),
            Work::Deliver {
                frame,
                seq,
                outcome,
                ..
            } => e.exec.deliver(&def, frame, seq, outcome, &env),
        };
        self.absorb(&mut e, &def, step);
        self.store(e);
    }

    /// Keeps a step's events, hands out its calls, and records its end.
    pub(crate) fn absorb(&mut self, e: &mut Execution, def: &Arc<Definition>, step: Step) {
        self.keep_events(e, &step.events);
        for d in step.dispatches {
            self.route(e, d);
        }
        if let Some(end) = step.end {
            self.end(e, end);
        } else if e.events > MOST_EVENTS {
            // (memory) AWS fails an execution that passes its history quota.
            let failure = aws_asl::Failure::runtime(
                "The execution reached the maximum number of history events (25000).",
            );
            let env = self.env_of(e);
            let (abort, _) = e
                .exec
                .abort(Some(&failure.error), Some(&failure.cause), &env);
            self.keep_events(e, &abort.events);
            self.end(e, End::Failed(failure));
        }
        let _ = def;
    }

    fn keep_events(&mut self, e: &mut Execution, events: &[aws_asl::Event]) {
        for ev in events {
            e.events += 1;
            if !e.express {
                self.history.put(
                    history_key(&e.arn, ev.id),
                    ev.wire().to_string().into_bytes(),
                );
            }
        }
        if !events.is_empty() {
            self.log_events(e, events);
            self.note_map_runs(e, events);
        }
    }

    /// Where a call goes: an activity's queue, a child execution, or a job for the world outside.
    /// The answer of a mocked state, when the execution runs a test case that mocks it.
    fn mocked(&self, e: &mut Execution, d: &Dispatch) -> Option<Outcome> {
        let case = e.test_case.as_deref()?;
        if d.resource == aws_asl::ITEM_READER || d.resource == aws_asl::exec::RESULT_WRITER {
            return None;
        }
        let machine = e.machine_arn.rsplit(':').next()?;
        let n = e
            .mock_calls
            .iter()
            .find(|(s, _)| *s == d.state)
            .map_or(0, |(_, n)| *n);
        let out = self.config.mocked(machine, case, &d.state, n)?;
        match e.mock_calls.iter_mut().find(|(s, _)| *s == d.state) {
            Some((_, count)) => *count += 1,
            None => e.mock_calls.push((d.state.clone(), 1)),
        }
        Some(out)
    }

    fn route(&mut self, e: &mut Execution, d: Dispatch) {
        if let Some(outcome) = self.mocked(e, &d) {
            self.queue.push_back(Work::Deliver {
                execution: e.arn.clone(),
                frame: d.frame,
                seq: d.seq,
                outcome,
            });
            return;
        }
        let base = d
            .resource
            .split(".waitForTaskToken")
            .next()
            .unwrap_or_default();
        if base.starts_with("arn:aws:states:") && base.contains(":activity:") {
            let activity = base.to_owned();
            e.activity.retain(|a| a.frame != d.frame);
            e.activity.push(ActivityCall {
                frame: d.frame,
                seq: d.seq,
                activity: activity.clone(),
                input: d.input.to_string(),
                token: d.token.clone().unwrap_or_default(),
                taken: false,
            });
            if !self.activities.iter().any(|(_, a)| a.arn == activity) {
                // (memory) An activity that does not exist fails the task.
                self.queue.push_back(Work::Deliver {
                    execution: e.arn.clone(),
                    frame: d.frame,
                    seq: d.seq,
                    outcome: Outcome::Failed(aws_asl::Failure::new(
                        "States.Runtime",
                        format!("The activity {activity} does not exist"),
                    )),
                });
                return;
            }
            self.effects.activities.push(activity);
            return;
        }
        if d.resource
            .starts_with("arn:aws:states:::states:startExecution")
        {
            self.start_child(e, &d);
            return;
        }
        let (region, account) = crate::arn::parts(&e.arn)
            .map(|p| (p.region.to_owned(), p.account.to_owned()))
            .unwrap_or_default();
        self.effects.jobs.push(Job {
            execution: e.arn.clone(),
            frame: d.frame,
            seq: d.seq,
            resource: d.resource,
            input: d.input,
            token: d.token,
            credentials: d.credentials,
            role_arn: e.role_arn.clone(),
            region,
            account,
            endpoint: e.endpoint.clone(),
        });
    }

    /// Records how an execution ended, tells whoever waits for it, and keeps the store bounded.
    pub(crate) fn end(&mut self, e: &mut Execution, end: End) {
        let now = self.now_ms();
        e.stop_ms = Some(now);
        e.activity.clear();
        match end {
            End::Succeeded(output) => {
                e.status = Run::Succeeded;
                e.output = Some(output);
            }
            End::Failed(f) => {
                e.status = Run::Failed;
                e.error = Some(f.error);
                e.cause = Some(f.cause);
            }
            End::TimedOut(f) => {
                e.status = Run::TimedOut;
                e.error = Some(f.error);
                e.cause = (!f.cause.is_empty()).then_some(f.cause);
            }
        }
        self.ended(e);
    }

    /// What every end shares (a stop included).
    pub(crate) fn ended(&mut self, e: &mut Execution) {
        self.close_runs(e, "ABORTED", true);
        if e.status == Run::Succeeded {
            e.exec.frames.clear();
        }
        self.effects.ended.push(crate::effects::Ended {
            arn: e.arn.clone(),
            answer: self.sync_answer(e),
        });
        self.notice(e);
        for (parent, frame, seq, sync2) in std::mem::take(&mut e.parents) {
            let outcome = crate::children::outcome_for_parent(e, sync2, self.now_ms());
            self.queue.push_back(Work::Deliver {
                execution: parent,
                frame,
                seq,
                outcome,
            });
        }
        self.trim(&e.machine_arn.clone(), &e.arn.clone());
    }

    /// Puts an execution back after a step. A finished Express execution is not kept: nothing can describe it (AWS keeps its
    /// history only in its log group).
    pub(crate) fn store(&mut self, e: Execution) {
        if e.express && e.status != Run::Running {
            self.executions.remove(&e.arn);
            return;
        }
        self.executions.insert(e.arn.clone(), e);
    }

    /// Keeps the newest executions of a machine and drops the oldest finished ones with their history.
    fn trim(&mut self, machine: &str, keep: &str) {
        const KEEP: usize = 1_000;
        let mut finished: Vec<(i64, String)> = self
            .executions
            .iter()
            .filter(|(_, x)| x.machine_arn == machine && x.status != Run::Running && x.arn != keep)
            .map(|(k, x)| (x.start_ms, k.to_owned()))
            .collect();
        if finished.len() < KEEP {
            return;
        }
        finished.sort();
        let drop = finished.len() + 1 - KEEP;
        for (_, k) in finished.into_iter().take(drop) {
            self.executions.remove(&k);
            self.history
                .delete_range(&format!("{k}/"), &format!("{k}0"));
        }
    }

    /// The next time anything is due, over every running execution.
    pub(crate) fn next_due(&self) -> Option<i64> {
        self.executions
            .iter()
            .filter(|(_, e)| e.status == Run::Running)
            .filter_map(|(_, e)| e.exec.next_due())
            .min()
    }

    /// A wake-up: everything due by `at` is woken.
    pub(crate) fn tick(&mut self, caller: &Caller, at: i64) {
        if self.last_caller.is_none() {
            self.last_caller = Some(caller.clone());
        }
        let now = self.now_ms().max(at);
        let due: Vec<String> = self
            .executions
            .iter()
            .filter(|(_, e)| {
                e.status == Run::Running && e.exec.next_due().is_some_and(|d| d <= now)
            })
            .map(|(k, _)| k.to_owned())
            .collect();
        self.queue.extend(due.into_iter().map(Work::Wake));
        self.process();
    }

    /// An answer to a task call.
    pub(crate) fn result(&mut self, execution: &str, frame: u32, seq: u32, outcome: Outcome) {
        self.queue.push_back(Work::Deliver {
            execution: execution.to_owned(),
            frame,
            seq,
            outcome,
        });
        self.process();
    }

    /// After a restart: the calls that were out are made again, the activities announced again.
    pub(crate) fn resume(&mut self, caller: &Caller) {
        self.last_caller = Some(caller.clone());
        let running: Vec<String> = self
            .executions
            .iter()
            .filter(|(_, e)| e.status == Run::Running)
            .map(|(k, _)| k.to_owned())
            .collect();
        for arn in running {
            let Some(mut e) = self.executions.get(&arn).cloned() else {
                continue;
            };
            let Some(def) = self.definition(&e.definition) else {
                continue;
            };
            for d in e.exec.calls(&def) {
                self.route(&mut e, d);
            }
            let pending: Vec<String> = e
                .activity
                .iter()
                .filter(|a| !a.taken)
                .map(|a| a.activity.clone())
                .collect();
            self.effects.activities.extend(pending);
            self.queue.push_back(Work::Run(arn.clone()));
            self.store(e);
        }
        self.process();
    }

    /// The `Step Functions Execution Status Change` event of an execution (memory: the detail's members).
    pub(crate) fn notice(&mut self, e: &Execution) {
        if e.express {
            return;
        }
        let Some(p) = crate::arn::parts(&e.arn) else {
            return;
        };
        let detail = json!({
            "executionArn": e.arn, "stateMachineArn": e.machine_arn, "name": e.name, "status": e.status.name(),
            "startDate": e.start_ms, "stopDate": e.stop_ms, "input": e.input, "output": e.output,
            "inputDetails": {"included": true}, "outputDetails": e.output.as_ref().map(|_| json!({"included": true})),
            "error": e.error, "cause": e.cause,
        });
        self.effects.notices.push((
            detail,
            e.arn.clone(),
            p.region.to_owned(),
            p.account.to_owned(),
        ));
    }
}

/// The row key of event `id` of an execution.
pub(crate) fn history_key(arn: &str, id: u64) -> String {
    format!("{arn}/{id:010}")
}
