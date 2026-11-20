//! An alarm moving to another state: its history, and the actions it queues.

use serde_json::json;

use crate::core::{Core, Outgoing};
use crate::model::{Item, MAX_HISTORY};

impl Core {
    /// Moves an alarm to `state`. A change is recorded in its history and, when its actions are enabled, queues them.
    pub(crate) fn move_to(
        &mut self,
        name: &str,
        state: &str,
        why: (&str, Option<&str>),
        now: i64,
        changed: bool,
    ) {
        let Some(a) = self.alarms.get_mut(name) else {
            return;
        };
        let old = std::mem::replace(&mut a.state, state.to_owned());
        a.reason = why.0.to_owned();
        a.reason_data = why.1.map(str::to_owned);
        a.state_updated_ms = now;
        if !changed {
            return;
        }
        a.transitioned_ms = now;
        let actions = if !a.actions_enabled {
            Vec::new()
        } else {
            match state {
                "ALARM" => a.alarm_actions.clone(),
                "OK" => a.ok_actions.clone(),
                _ => a.insufficient_actions.clone(),
            }
        };
        let (alarm_type, reason, data) = (
            crate::alarms::type_of(a.kind),
            a.reason.clone(),
            a.reason_data.clone(),
        );
        let history = json!({
            "version": "1.0",
            "oldState": {"stateValue": old},
            "newState": {"stateValue": state, "stateReason": reason, "stateReasonData": data},
        })
        .to_string();
        self.record(
            name,
            now,
            (
                "StateUpdate",
                &format!("Alarm updated from {old} to {state}"),
                &history,
                alarm_type,
            ),
        );
        for action in actions {
            self.outbox.push(Outgoing {
                alarm: name.to_owned(),
                action,
                old: old.clone(),
                new: state.to_owned(),
                at_ms: now,
            });
        }
        self.cascade(name);
    }

    /// Adds an entry to an alarm's history, keeping the newest [`MAX_HISTORY`].
    pub(crate) fn record(&mut self, name: &str, at_ms: i64, entry: (&str, &str, &str, &str)) {
        let item = Item {
            at_ms,
            kind: entry.0.to_owned(),
            summary: entry.1.to_owned(),
            data: entry.2.to_owned(),
            alarm_type: entry.3.to_owned(),
        };
        if self.history.get(name).is_none() {
            self.history
                .insert(name.to_owned(), crate::model::History::default());
        }
        if let Some(h) = self.history.get_mut(name) {
            h.items.push(item);
            let excess = h.items.len().saturating_sub(MAX_HISTORY);
            h.items.drain(..excess);
        }
    }
}
