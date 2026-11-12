//! Account settings: the minimum throughput billing commitment. It is stored, and it moves through the states the model names
//! (`ENABLED`, `ENABLED_UNTIL_EARLIEST_ALLOWED_END`, `DISABLED`); it bills nothing, and nothing local reads it. The rules are from
//! the model's own documentation and the wording of the states is its. (memory: no recording of this operation exists)

use aws_wire::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Put, Req, secs};

/// The key of the one row.
const KEY: &str = "minimum-throughput-billing-commitment";
/// A commitment cannot be ended before this long after it began ("a minimum duration of 24 hours" in the model).
const MINIMUM_MS: i64 = 24 * 3_600_000;

/// Where a commitment stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum State {
    Enabled,
    /// A disable was asked for before the earliest allowed end: it takes effect then, and enabling again cancels it.
    Disabling,
    Disabled,
}

/// The commitment of the account in this region.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Commitment {
    pub(crate) state: State,
    pub(crate) started_ms: i64,
    pub(crate) earliest_end_ms: i64,
    pub(crate) ended_ms: Option<i64>,
}

impl Commitment {
    /// The commitment as it stands at `now_ms`: a pending disable whose time has come has taken effect.
    fn at(&self, now_ms: i64) -> Self {
        let mut c = self.clone();
        if c.state == State::Disabling && now_ms >= c.earliest_end_ms {
            c.state = State::Disabled;
            c.ended_ms = Some(c.earliest_end_ms);
        }
        c
    }

    fn json(&self) -> Value {
        let mut v = json!({
            "Status": match self.state {
                State::Enabled => "ENABLED",
                State::Disabling => "ENABLED_UNTIL_EARLIEST_ALLOWED_END",
                State::Disabled => "DISABLED",
            },
            "StartedAt": secs(self.started_ms),
            "EarliestAllowedEndAt": secs(self.earliest_end_ms),
        });
        if let Some(ended) = self.ended_ms {
            v.put("EndedAt", secs(ended));
        }
        v
    }
}

impl Core {
    fn commitment_json(&self) -> Value {
        match self.account.get(KEY) {
            Some(c) => c.at(self.now_ms()).json(),
            None => json!({"Status": "DISABLED"}),
        }
    }

    pub(crate) fn describe_account_settings(&self) -> Value {
        json!({"MinimumThroughputBillingCommitment": self.commitment_json()})
    }

    pub(crate) fn update_account_settings(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let enable =
            r.v.pointer("/MinimumThroughputBillingCommitment/Status")
                .and_then(Value::as_str)
                == Some("ENABLED");
        let now = self.now_ms();
        let current = self.account.get(KEY).map(|c| c.at(now));
        let next = match (current, enable) {
            // Enabling begins a commitment, or cancels a pending disable of the one that runs.
            (Some(c), true) if c.state != State::Disabled => Commitment {
                state: State::Enabled,
                ..c
            },
            (_, true) => Commitment {
                state: State::Enabled,
                started_ms: now,
                earliest_end_ms: now + MINIMUM_MS,
                ended_ms: None,
            },
            // Disabling before the earliest allowed end schedules it; after, it takes effect.
            (Some(c), false) if c.state == State::Enabled => {
                if now >= c.earliest_end_ms {
                    Commitment {
                        state: State::Disabled,
                        ended_ms: Some(now),
                        ..c
                    }
                } else {
                    Commitment {
                        state: State::Disabling,
                        ..c
                    }
                }
            }
            (Some(c), false) => c,
            (None, false) => {
                return Ok(json!({"MinimumThroughputBillingCommitment": {"Status": "DISABLED"}}));
            }
        };
        let out = next.json();
        self.account.insert(KEY.to_owned(), next);
        Ok(json!({"MinimumThroughputBillingCommitment": out}))
    }
}
