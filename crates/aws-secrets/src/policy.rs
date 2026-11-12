//! A secret's resource policy: stored and returned as given. It is not evaluated until the IAM stage can; `ValidateResourcePolicy`
//! checks that the text is a JSON object and nothing more.

use aws_model_secrets_manager::keys::{
    DeleteResourcePolicyRequest as Delete, DeleteResourcePolicyResponse as Deleted,
    GetResourcePolicyRequest as Get, GetResourcePolicyResponse as Got,
    PutResourcePolicyRequest as Put, PutResourcePolicyResponse as Put_,
    ValidateResourcePolicyRequest as Validate, ValidateResourcePolicyResponse as Validated,
};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, not_found};

// fuzz:parser fuzz_resource_policy
/// Whether `text` is a JSON object.
pub(crate) fn is_policy(text: &str) -> bool {
    matches!(
        serde_json::from_str::<serde_json::Value>(text),
        Ok(serde_json::Value::Object(_))
    )
}

fn malformed() -> ApiError {
    ApiError::modeled(
        "MalformedPolicyDocumentException",
        "This resource policy contains invalid JSON.",
    )
}

impl Core {
    pub(crate) fn put_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Put>(call)?;
        let name = self.locate_live(v.get(Put::SECRET_ID).unwrap_or_default())?;
        let text = v.get(Put::RESOURCE_POLICY).unwrap_or_default();
        if !is_policy(text) {
            return Err(malformed());
        }
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        secret.policy = Some(text.to_owned());
        let arn = secret.arn.clone();
        reply::<Put_>(|o| {
            o.put(Put_::ARN, &arn);
            o.put(Put_::NAME, &name);
        })
    }

    pub(crate) fn get_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let s = self.secret(v.get(Get::SECRET_ID).unwrap_or_default())?;
        reply::<Got>(|o| {
            o.put(Got::ARN, &s.arn);
            o.put(Got::NAME, &s.name);
            if let Some(p) = &s.policy {
                o.put(Got::RESOURCE_POLICY, p);
            }
        })
    }

    pub(crate) fn delete_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let name = self.locate_live(v.get(Delete::SECRET_ID).unwrap_or_default())?;
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        secret.policy = None;
        let arn = secret.arn.clone();
        reply::<Deleted>(|o| {
            o.put(Deleted::ARN, &arn);
            o.put(Deleted::NAME, &name);
        })
    }

    pub(crate) fn validate_resource_policy(&mut self, call: &Call) -> Reply {
        let v = request::<Validate>(call)?;
        if !is_policy(v.get(Validate::RESOURCE_POLICY).unwrap_or_default()) {
            return Err(malformed());
        }
        reply::<Validated>(|o| {
            o.put_bool(Validated::POLICY_VALIDATION_PASSED, true);
            o.put_list(Validated::VALIDATION_ERRORS, |_| {});
        })
    }
}

#[cfg(test)]
mod tests {
    use super::is_policy;

    #[test]
    fn a_policy_is_a_balanced_json_object() {
        assert!(is_policy(
            r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow"}]}"#
        ));
        assert!(!is_policy("[]"));
        assert!(!is_policy(r#"{"a":"#));
        assert!(!is_policy(r#"{"a":]}"#));
        assert!(is_policy(r#"{"a":"}"}"#));
    }
}
