//! `/aws/reference/secretsmanager/<secret id>`: a Secrets Manager secret read through Parameter Store. Only `GetParameter` and
//! `GetParameters` read it, and only with `WithDecryption` (a secret is never shown sealed). It is a `SecureString` of version 0 whose
//! value is the secret's current `SecretString` (a binary secret comes back as base64). The words of the refusal and the version
//! number are not recorded.

use std::collections::BTreeSet;

use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Value, json};

use crate::core::{Core, validation};
use crate::model::{Kind, Version};
use crate::write::not_found;

impl Core {
    /// The secret `secret` names, as a parameter version and its value.
    pub(crate) async fn reference(
        &self,
        call: &Call,
        secret: &str,
        decrypt: bool,
    ) -> Result<(Version, String), ApiError> {
        if !decrypt {
            // (memory)
            return Err(validation(
                "You must set WithDecryption to true to read a Secrets Manager secret through Parameter Store.",
            ));
        }
        let peers = self
            .peers
            .as_ref()
            .ok_or_else(|| ApiError::internal("Secrets Manager is not running"))?;
        let got = peers
            .call(
                "ssm",
                &call.caller,
                "secretsmanager",
                "GetSecretValue",
                &json!({"SecretId": secret}),
            )
            .await
            .map_err(|e| {
                if e.shape.as_deref() == Some("ResourceNotFoundException") {
                    not_found()
                } else {
                    e
                }
            })?;
        let value = match got.get("SecretString").and_then(Value::as_str) {
            Some(s) => s.to_owned(),
            None => got
                .get("SecretBinary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        };
        #[allow(clippy::cast_possible_truncation)]
        let modified_ms = (got
            .get("CreatedDate")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            * 1000.0) as i64;
        let version = Version {
            number: 0,
            value: Vec::new(),
            kind: Kind::SecureString,
            key_id: None,
            description: None,
            allowed_pattern: None,
            data_type: "text".to_owned(),
            advanced: false,
            policies: Vec::new(),
            labels: BTreeSet::new(),
            modified_ms,
            modified_by: String::new(),
        };
        Ok((version, value))
    }
}
