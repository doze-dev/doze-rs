//! Secrets Manager's use of KMS: a secret's value is kept under a KMS key by envelope encryption (see [`aws_peers::envelope`]), with
//! the secret's ARN and the version's id as the encryption context (`SecretARN`, `SecretVersionId`, as Secrets Manager does). The key
//! is the one the secret names, or `alias/aws/secretsmanager`.

use aws_peers::envelope;
use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::Secret;

/// The key a secret is sealed with when it names none.
pub(crate) const DEFAULT_KEY: &str = "alias/aws/secretsmanager";

fn context(arn: &str, version: &str) -> Value {
    json!({"SecretARN": arn, "SecretVersionId": version})
}

/// What went wrong with KMS, in Secrets Manager's words.
fn failure(code: &'static str, e: ApiError) -> ApiError {
    if e.status >= 500 || e.code == "InternalFailure" {
        return e;
    }
    ApiError::modeled(code, e.message)
}

fn no_kms() -> ApiError {
    ApiError::internal("a secret needs KMS, which is not running")
}

impl Core {
    /// Seals `plain` as version `version` of `secret`.
    pub(crate) async fn kms_seal(
        &self,
        call: &Call,
        secret: &Secret,
        version: &str,
        plain: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(no_kms)?;
        let key = secret.kms_key_id.as_deref().unwrap_or(DEFAULT_KEY);
        envelope::seal(
            peers,
            "secretsmanager",
            &call.caller,
            key,
            &context(&secret.arn, version),
            plain,
        )
        .await
        .map_err(|e| failure("EncryptionFailure", e))
    }

    /// Opens what [`Core::kms_seal`] made.
    pub(crate) async fn kms_open(
        &self,
        call: &Call,
        secret: &Secret,
        version: &str,
        stored: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(no_kms)?;
        envelope::open(
            peers,
            "secretsmanager",
            &call.caller,
            &context(&secret.arn, version),
            stored,
        )
        .await
        .map_err(|e| failure("DecryptionFailure", e))
    }
}
