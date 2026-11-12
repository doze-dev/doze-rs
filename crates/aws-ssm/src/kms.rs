//! SSM's use of KMS: a `SecureString` is kept under a KMS key by envelope encryption (see [`aws_peers::envelope`]), with the
//! parameter's ARN as the encryption context (`PARAMETER_ARN`, as SSM does). The key is the one the parameter names, or
//! `alias/aws/ssm`.

use aws_peers::envelope;
use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::json;

use crate::core::Core;

/// The key a `SecureString` is sealed with when its writer names none.
pub(crate) const DEFAULT_KEY: &str = "alias/aws/ssm";

/// What went wrong with KMS, in SSM's words (the code is SSM's; the message is KMS's).
fn invalid_key(e: ApiError) -> ApiError {
    if e.status >= 500 || e.code == "InternalFailure" {
        return e;
    }
    ApiError::modeled("InvalidKeyId", e.message)
}

impl Core {
    /// Seals `plain` under `key_id` for the parameter whose ARN is `arn`.
    pub(crate) async fn kms_seal(
        &self,
        call: &Call,
        key_id: &str,
        arn: &str,
        plain: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(no_kms)?;
        envelope::seal(
            peers,
            "ssm",
            &call.caller,
            key_id,
            &json!({"PARAMETER_ARN": arn}),
            plain,
        )
        .await
        .map_err(invalid_key)
    }

    /// Opens what [`Core::kms_seal`] made.
    pub(crate) async fn kms_open(
        &self,
        call: &Call,
        arn: &str,
        stored: &[u8],
    ) -> Result<Vec<u8>, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(no_kms)?;
        envelope::open(
            peers,
            "ssm",
            &call.caller,
            &json!({"PARAMETER_ARN": arn}),
            stored,
        )
        .await
        .map_err(invalid_key)
    }
}

fn no_kms() -> ApiError {
    ApiError::internal("a SecureString needs KMS, which is not running")
}
