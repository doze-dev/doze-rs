//! A key's policy: stored and returned as given, and evaluated by IAM as the key's resource policy on every request that names the key
//! (`authz.rs`). `default` is the only policy name.

use aws_model_kms::keys::{
    GetKeyPolicyRequest as Get, GetKeyPolicyResponse as Got, ListKeyPoliciesRequest as List,
    ListKeyPoliciesResponse as Listed, PutKeyPolicyRequest as Put,
};
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::Core;
use crate::find::not_found_key;
use crate::keys::invalid_state;
use crate::model::State;

const NAME: &str = "default";

// fuzz:parser fuzz_key_policy
/// Whether `text` is a JSON object.
pub(crate) fn is_policy(text: &str) -> bool {
    matches!(
        serde_json::from_str::<serde_json::Value>(text),
        Ok(serde_json::Value::Object(_))
    )
}

/// Only `default` is a policy name (AWS's words, recorded 2026-11-15).
fn bad_name() -> ApiError {
    crate::core::validation("Invalid policy name")
}

impl Core {
    pub(crate) fn get_key_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let name = v.get(Get::POLICY_NAME).unwrap_or(NAME);
        let key = self.key(call, v.get(Get::KEY_ID).unwrap_or_default())?;
        if name != NAME {
            return Err(bad_name());
        }
        reply::<Got>(|o| {
            o.put(Got::POLICY, key.policy.as_deref().unwrap_or_default());
            o.put(Got::POLICY_NAME, NAME);
        })
    }

    pub(crate) fn put_key_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Put>(call)?;
        let name = v.get(Put::POLICY_NAME).unwrap_or(NAME);
        let text = v.get(Put::POLICY).unwrap_or_default();
        let id = self.locate(call, v.get(Put::KEY_ID).unwrap_or_default())?;
        if name != NAME {
            return Err(bad_name());
        }
        if !is_policy(text) {
            // AWS's message is empty (recorded 2026-11-15).
            return Err(ApiError::modeled("MalformedPolicyDocumentException", ""));
        }
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        key.policy = Some(text.to_owned());
        empty()
    }

    pub(crate) fn list_key_policies(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        self.key(call, v.get(List::KEY_ID).unwrap_or_default())?;
        reply::<Listed>(|o| {
            o.put_list(Listed::POLICY_NAMES, |l| {
                l.push(NAME);
            });
            o.put_bool(Listed::TRUNCATED, false);
        })
    }
}
