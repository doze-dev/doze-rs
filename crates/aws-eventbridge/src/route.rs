//! Sending an event through a bus: archive it, find the enabled rules whose pattern matches it, and deliver it to each of their
//! targets. Delivery is in the call (the answer waits for it, as it did in the Go edition), except to another region, which is handed
//! to a task so two regions never wait on each other, and to an API destination, whose HTTP call is a task too (`deliver_http.rs`). An
//! event bus target of this region puts the event on that bus too, up to a
//! limit that stops a loop.

use std::collections::VecDeque;

use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::core::Core;
use crate::deliver::{Delivery, Outcome};
use crate::event::Event;
use crate::input::{self, Names};
use crate::model::key;
use crate::pattern;
use crate::req::no_bus;
use crate::util::iso;

/// How many buses one event may be forwarded across before the rest are dropped (a bus that targets itself would otherwise never end).
const MAX_FORWARDS: usize = 32;

impl Core {
    /// Puts `event` on bus `bus` of `account` in `region`.
    ///
    /// # Errors
    /// `ResourceNotFoundException` when the bus is not there.
    pub(crate) async fn send_to_bus(
        &mut self,
        caller: &Caller,
        region: &str,
        account: &str,
        bus: &str,
        event: Event,
    ) -> Result<(), ApiError> {
        if region != caller.region {
            self.forward_to_region(caller, region, account, bus, &event);
            return Ok(());
        }
        self.ensure_default(account, region);
        if self.buses.get(&key(account, &[bus])).is_none() {
            return Err(no_bus(bus));
        }
        self.publish(caller, account, bus, event, None).await;
        Ok(())
    }

    /// Runs the event through the bus and every bus it is forwarded to. `only` limits delivery to the rules with those ARNs (a replay
    /// that names rules).
    pub(crate) async fn publish(
        &mut self,
        caller: &Caller,
        account: &str,
        bus: &str,
        event: Event,
        only: Option<&[String]>,
    ) {
        let mut queue = VecDeque::from([(account.to_owned(), bus.to_owned(), event)]);
        let mut forwards = 0;
        while let Some((account, bus, event)) = queue.pop_front() {
            self.archive_event(&account, &bus, &event);
            for d in self.matching(&account, &bus, &event, only) {
                let Some(next) = self.attempt(d, 1, caller).await else {
                    continue;
                };
                if forwards < MAX_FORWARDS {
                    forwards += 1;
                    queue.push_back(next);
                } else {
                    tracing::warn!("an event reached the forwarding limit and was not sent on");
                }
            }
        }
    }

    /// Sends one delivery. A target that is an event bus of this region comes back as the bus to put the event on next.
    pub(crate) async fn attempt(
        &mut self,
        d: Delivery,
        tries: u32,
        caller: &Caller,
    ) -> Option<(String, String, Event)> {
        match self.send(&d, caller).await {
            Outcome::Sent | Outcome::Handed => None,
            Outcome::Forward {
                region,
                account,
                bus,
            } if region == caller.region => Some((account, bus, d.event)),
            Outcome::Forward {
                region,
                account,
                bus,
            } => {
                self.forward_to_region(caller, &region, &account, &bus, &d.event);
                None
            }
            Outcome::NotLocal(why) => {
                tracing::warn!(rule = %d.rule_arn, target = %d.target.id, "not delivered: {why}");
                None
            }
            Outcome::Failed(why) => {
                self.failed(d, why, tries, caller).await;
                None
            }
        }
    }

    /// The deliveries of `event` to the enabled rules of the bus whose pattern it matches.
    fn matching(
        &mut self,
        account: &str,
        bus: &str,
        event: &Event,
        only: Option<&[String]>,
    ) -> Vec<Delivery> {
        let keys: Vec<String> = self
            .rules
            .with_prefix(&key(account, &[bus, ""]))
            .filter(|(_, r)| r.enabled && r.pattern.is_some() && !r.targets.is_empty())
            .filter(|(_, r)| only.is_none_or(|o| o.contains(&r.arn)))
            .map(|(k, _)| k.to_owned())
            .collect();
        let doc = event.doc();
        let text = event.text();
        let mut out = Vec::new();
        for k in keys {
            if self
                .pattern_of(&k)
                .is_some_and(|p| pattern::matches(p, &doc))
            {
                out.extend(self.deliveries_for(&k, event, &doc, &text));
            }
        }
        out
    }

    /// One delivery per target of the rule. A target whose input settings leave nothing to send is skipped.
    pub(crate) fn deliveries_for(
        &self,
        rule_key: &str,
        event: &Event,
        doc: &Value,
        text: &str,
    ) -> Vec<Delivery> {
        let Some(rule) = self.rules.get(rule_key) else {
            return Vec::new();
        };
        let ingestion = iso(self.now_ms());
        let names = Names {
            rule_arn: &rule.arn,
            rule_name: &rule.name,
            ingestion_time: &ingestion,
        };
        rule.targets
            .iter()
            .filter_map(|t| {
                let spec: Value = serde_json::from_str(&t.json).unwrap_or(Value::Null);
                let Some(payload) = input::shape(&spec, doc, text, &names) else {
                    tracing::warn!(rule = %rule.arn, target = %t.id, "the target's InputPath found nothing in the event; nothing was sent");
                    return None;
                };
                Some(Delivery {
                    rule_arn: rule.arn.clone(),
                    rule_name: rule.name.clone(),
                    target: t.clone(),
                    spec,
                    payload,
                    event: event.clone(),
                })
            })
            .collect()
    }

    /// Hands the event to the service of another region as a `PutEvents` from EventBridge, off this actor.
    fn forward_to_region(
        &self,
        caller: &Caller,
        region: &str,
        account: &str,
        bus: &str,
        event: &Event,
    ) {
        let Some(peers) = self.peers.clone() else {
            return;
        };
        let mut as_there = caller.clone();
        as_there.region = region.to_owned();
        as_there.account_id = account.to_owned();
        let entry = json!({"Entries": [{
            "EventBusName": bus, "Source": event.source, "DetailType": event.detail_type,
            "Detail": event.detail.to_string(), "Resources": event.resources,
            "Time": event.time_ms as f64 / 1000.0,
        }]});
        self.tasks.spawn(async move {
            if let Err(e) = peers
                .call("events", &as_there, "events", "PutEvents", &entry)
                .await
            {
                tracing::warn!(
                    "EventBridge could not forward an event to another region: {}",
                    e.message
                );
            }
        });
    }
}
