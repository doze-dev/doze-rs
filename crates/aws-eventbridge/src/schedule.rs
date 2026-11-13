//! Schedules and retries, driven by deadlines. After every call the actor works out the earliest time anything needs doing (a
//! scheduled rule's next fire, a failed delivery's next try); the timer task sleeps to it, asks the actor to do what is due, and is
//! told the next deadline. With nothing scheduled there is no deadline, no timer and no task.
//!
//! A schedule fires at the first slot after the moment it was first seen. What a restart or a long pause missed is not replayed
//! (AWS does not either). A rate counts from the rule's creation, so its slots do not move when the process restarts.

use aws_wire::service::Caller;
use serde_json::json;

use crate::core::Core;
use crate::event::Event;
use crate::model::Rule;

impl Core {
    /// Works out the next fire after `from` of every enabled scheduled rule that has a target and has none yet; forgets those that no
    /// longer apply.
    fn refresh_due(&mut self, from: i64) {
        let live: Vec<(String, i64)> = self
            .rules
            .iter()
            .filter(|(_, r)| is_live(r))
            .filter_map(|(k, r)| {
                let schedule = aws_cron::parse(r.schedule.as_deref()?).ok()?;
                Some((k.to_owned(), schedule.next_after(r.anchor_ms, from)?))
            })
            .collect();
        self.due.retain(|k, _| live.iter().any(|(l, _)| l == k));
        for (k, at) in live {
            self.due.entry(k).or_insert(at);
        }
    }

    /// The earliest time anything is due: a schedule, or a retry.
    pub(crate) fn next_due(&mut self) -> Option<i64> {
        self.refresh_due(self.now_ms());
        let schedules = self.due.values().min().copied();
        [schedules, self.next_retry()].into_iter().flatten().min()
    }

    /// Does what is due: fires the rules whose time has come, and tries again the deliveries whose wait is over.
    pub(crate) async fn tick(&mut self, armed_by: &Caller, deadline: i64) {
        let caller = self.last_caller.clone().unwrap_or_else(|| armed_by.clone());
        // A service that was reaped and opened again for this wake-up has no memory of what was due: the slot it was woken for is the
        // first one after just before the deadline.
        self.refresh_due(deadline.saturating_sub(1).min(self.now_ms()));
        self.fire_due(&caller).await;
        self.retry_due(&caller).await;
    }

    async fn fire_due(&mut self, caller: &Caller) {
        let now = self.now_ms();
        let due: Vec<(String, i64)> = self
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(k, at)| (k.clone(), *at))
            .collect();
        for (k, at) in due {
            let Some(rule) = self.rules.get(&k) else {
                continue;
            };
            let next = rule
                .schedule
                .as_deref()
                .and_then(|s| aws_cron::parse(s).ok())
                .and_then(|s| s.next_after(rule.anchor_ms, now.max(at)));
            let event = scheduled_event(rule, caller, at);
            // The delivery is made for the rule's account, whoever armed the timer.
            let who = Caller {
                account_id: rule.account.clone(),
                ..caller.clone()
            };
            match next {
                Some(n) => {
                    self.due.insert(k.clone(), n);
                }
                None => {
                    self.due.remove(&k);
                }
            }
            let (doc, text) = (event.doc(), event.text());
            for d in self.deliveries_for(&k, &event, &doc, &text) {
                // A scheduled event goes to its own rule only; a target that is another bus gets it on that bus.
                if let Some((account, bus, e)) = self.attempt(d, 1, &who).await {
                    self.publish(&who, &account, &bus, e, None).await;
                }
            }
        }
    }

    async fn retry_due(&mut self, caller: &Caller) {
        let now = self.now_ms();
        let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.retries)
            .into_iter()
            .partition(|r| r.due_ms <= now);
        self.retries = waiting;
        for r in due {
            let tries = r.tries + 1;
            let who = r.caller;
            if let Some((account, bus, e)) = self.attempt(r.delivery, tries, &who).await {
                self.publish(caller, &account, &bus, e, None).await;
            }
        }
    }
}

/// A rule that will fire: enabled, on a schedule, with somewhere to send.
fn is_live(r: &Rule) -> bool {
    r.enabled && r.schedule.is_some() && !r.targets.is_empty()
}

/// The event a schedule puts on the default bus: `Scheduled Event` from `aws.events`, naming the rule, with the time it was due.
fn scheduled_event(rule: &Rule, caller: &Caller, at: i64) -> Event {
    Event {
        id: aws_wire::RequestId::new().to_string(),
        source: "aws.events".to_owned(),
        detail_type: "Scheduled Event".to_owned(),
        account: rule.account.clone(),
        region: caller.region.clone(),
        time_ms: at,
        resources: vec![rule.arn.clone()],
        detail: json!({}),
        replay_name: None,
    }
}
