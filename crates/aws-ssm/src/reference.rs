//! `/aws/reference/secretsmanager/<secret id>`: a Secrets Manager secret read through Parameter Store. Only `GetParameter` and
//! `GetParameters` read it, and only with `WithDecryption` (a secret is never shown sealed); `GetParametersByPath` refuses the
//! path. What AWS answers was recorded on 2026-11-15: the parameter's ARN is the secret's, it has no `DataType`, and
//! `SourceResult` is the secret as JSON (its ARN, name, version, value, stages, and creation date in Java's medium form). The
//! version number AWS gives is not compared (the scenario holds it opaque); here it is 0.

use std::collections::BTreeSet;

use aws_wire::ApiError;
use aws_wire::service::Call;
use serde_json::{Value, json};

use crate::core::{Core, validation};
use crate::model::{Kind, Version};

/// A secret, as Parameter Store shows it.
pub(crate) struct Referenced {
    pub(crate) version: Version,
    pub(crate) value: String,
    /// The secret's ARN, which is the parameter's.
    pub(crate) arn: String,
    /// `SourceResult`: the secret as JSON.
    pub(crate) source: String,
}

impl Core {
    /// The secret `secret` names, as a parameter version and its value.
    pub(crate) async fn reference(
        &self,
        call: &Call,
        secret: &str,
        decrypt: bool,
    ) -> Result<Referenced, ApiError> {
        if !decrypt {
            return Err(validation(
                "WithDecryption flag must be True for retrieving a Secret Manager secret.",
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
                    absent(secret)
                } else {
                    e
                }
            })?;
        let text = |k: &str| got.get(k).and_then(Value::as_str).unwrap_or_default();
        let (value, value_key) = match got.get("SecretString").and_then(Value::as_str) {
            Some(s) => (s.to_owned(), "secretString"),
            None => (text("SecretBinary").to_owned(), "secretBinary"),
        };
        #[allow(clippy::cast_possible_truncation)]
        let created_secs = got
            .get("CreatedDate")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as i64;
        let stages = got.get("VersionStages").cloned().unwrap_or(json!([]));
        // Written by hand: AWS's members come in this order, and a map would sort them.
        let source = format!(
            "{{\"ARN\":{},\"name\":{},\"versionId\":{},\"{value_key}\":{},\"versionStages\":{stages},\"createdDate\":{}}}",
            json!(text("ARN")),
            json!(text("Name")),
            json!(text("VersionId")),
            json!(value),
            json!(medium_date(created_secs)),
        );
        let version = Version {
            number: 0,
            value: Vec::new(),
            kind: Kind::SecureString,
            key_id: None,
            description: None,
            allowed_pattern: None,
            data_type: String::new(),
            advanced: false,
            policies: Vec::new(),
            labels: BTreeSet::new(),
            modified_ms: created_secs * 1000,
            modified_by: String::new(),
        };
        Ok(Referenced {
            version,
            value,
            arn: text("ARN").to_owned(),
            source,
        })
    }
}

/// The refusal for a secret that is not there, in AWS's words: the name without its leading slash.
fn absent(secret: &str) -> ApiError {
    ApiError::modeled(
        "ParameterNotFound",
        format!(
            "An error occurred (ParameterNotFound) when referencing Secrets Manager: Secret aws/reference/secretsmanager/{secret} not found. Secrets Manager can't find the specified secret."
        ),
    )
}

/// `Nov 14, 2026, 8:53:14 PM`: Java's medium date and time, in UTC.
fn medium_date(secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (y, m, d) = aws_core::time::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    let (h, min, s) = (t / 3600, t % 3600 / 60, t % 60);
    let (h12, half) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    let month = usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("Jan");
    format!("{month} {d}, {y}, {h12}:{min:02}:{s:02} {half}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_written_as_java_writes_it() {
        // 2026-11-14T20:53:14Z
        assert_eq!(medium_date(1_794_689_594), "Nov 14, 2026, 8:53:14 PM");
        assert_eq!(medium_date(0), "Jan 1, 1970, 12:00:00 AM");
    }
}
