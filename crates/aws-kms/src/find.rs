//! Finding a key by the id a request gives: its id, its ARN, an alias, or an alias ARN. And whether it may be used.

use aws_wire::ApiError;
use aws_wire::service::Call;

use crate::core::Core;
use crate::model::{Key, State};
use crate::spec::Usage;

pub(crate) fn alias_arn(call: &Call, name: &str) -> String {
    format!(
        "arn:aws:kms:{}:{}:{name}",
        call.caller.region, call.caller.account_id
    )
}

pub(crate) fn key_arn(call: &Call, id: &str) -> String {
    format!(
        "arn:aws:kms:{}:{}:key/{id}",
        call.caller.region, call.caller.account_id
    )
}

pub(crate) fn not_found_key(call: &Call, id: &str) -> ApiError {
    ApiError::modeled(
        "NotFoundException",
        format!("Key '{}' does not exist", key_arn(call, id)),
    )
}

pub(crate) fn not_found_alias(call: &Call, name: &str) -> ApiError {
    ApiError::modeled(
        "NotFoundException",
        format!("Alias {} is not found.", alias_arn(call, name)),
    )
}

impl Core {
    /// The id of the key `given` names: a key id, a key ARN, `alias/name`, or an alias ARN.
    pub(crate) fn locate(&self, call: &Call, given: &str) -> Result<String, ApiError> {
        let tail = match given.strip_prefix("arn:") {
            Some(rest) => rest.rsplit_once(':').map_or(rest, |(_, t)| t),
            None => given,
        };
        if tail.starts_with("alias/") {
            return self
                .aliases
                .get(tail)
                .map(|a| a.key_id.clone())
                .ok_or_else(|| not_found_alias(call, tail));
        }
        let id = tail.strip_prefix("key/").unwrap_or(tail);
        if self.keys.get(id).is_some() {
            Ok(id.to_owned())
        } else {
            Err(not_found_key(call, id))
        }
    }

    /// The key `given` names.
    pub(crate) fn key(&self, call: &Call, given: &str) -> Result<&Key, ApiError> {
        let id = self.locate(call, given)?;
        self.keys.get(&id).ok_or_else(|| not_found_key(call, &id))
    }
}

/// A key that can be used for `operation`, which needs `usage`. Words recorded 2026-11-09.
pub(crate) fn usable(key: &Key, usage: Usage, operation: &str) -> Result<(), ApiError> {
    match key.state {
        State::PendingDeletion => {
            return Err(ApiError::modeled(
                "KMSInvalidStateException",
                format!("{} is pending deletion.", key.arn),
            ));
        }
        State::Disabled => {
            return Err(ApiError::modeled(
                "DisabledException",
                format!("{} is disabled.", key.arn),
            ));
        }
        State::Enabled => {}
    }
    if key.usage != usage {
        return Err(ApiError::modeled(
            "InvalidKeyUsageException",
            format!(
                "{} key usage is {} which is not valid for {operation}.",
                key.arn,
                key.usage.name()
            ),
        ));
    }
    Ok(())
}
