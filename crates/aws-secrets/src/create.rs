//! Making secrets and their versions: `CreateSecret`, `PutSecretValue`, `UpdateSecret`.

use std::collections::BTreeSet;

use aws_model_secrets_manager::keys::{
    CreateSecretRequest as Create, CreateSecretResponse as Created, PutSecretValueRequest as Put,
    PutSecretValueResponse as Put_, Tag, UpdateSecretRequest as Update,
    UpdateSecretResponse as Updated,
};
use aws_seal::Sealer;
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_parameter, invalid_request};
use crate::model::{CURRENT, DEPRECATED_KEPT_MS, MAX_VERSIONS, PREVIOUS, Secret, Version};

const MAX_TAGS: usize = 50;
const NAME_RULE: &str = "Invalid name. Must be a valid name containing alphanumeric characters, or any of the following: -/_+=.@!";

/// A value as given: its bytes, and whether it came as `SecretBinary`.
pub(crate) struct Value(pub(crate) Vec<u8>, pub(crate) bool);

/// The value a request carries. One or the other, never both (recorded 2026-11-09).
pub(crate) fn value_of(
    string: Option<&str>,
    binary: Option<&[u8]>,
) -> Result<Option<Value>, ApiError> {
    match (string, binary) {
        (Some(_), Some(_)) => Err(invalid_parameter(
            "You can't specify both a binary secret value and a string secret value in the same secret.",
        )),
        (Some(s), None) => Ok(Some(Value(s.as_bytes().to_vec(), false))),
        (None, Some(b)) => Ok(Some(Value(b.to_vec(), true))),
        (None, None) => Ok(None),
    }
}

fn check_name(name: &str) -> Result<(), ApiError> {
    let ok = (1..=512).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+=.@!-".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(ApiError::modeled("ValidationException", NAME_RULE))
    }
}

/// Adds a version holding `value` to `secret`. It gets `stages` (or `AWSCURRENT`); a stage lives on one version, so it
/// leaves the one that had it, and a version that loses `AWSCURRENT` to the new one becomes `AWSPREVIOUS`. Returns the
/// version id: the caller's token when it gave one, and then asking again with the same value is the same version.
pub(crate) fn add_version(
    sealer: &Sealer,
    secret: &mut Secret,
    token: Option<&str>,
    value: &Value,
    stages: Option<Vec<String>>,
    now: i64,
) -> Result<String, ApiError> {
    let id = match token {
        Some(t) => t.to_owned(),
        None => aws_seal::uuid_v4().ok_or_else(|| ApiError::internal("no randomness"))?,
    };
    let sealed = sealer
        .seal(&value.0)
        .ok_or_else(|| ApiError::internal("the value could not be sealed"))?;
    if let Some(same) = secret.versions.get(&id) {
        let before = sealer.open(&same.sealed);
        if before.as_deref() == Some(value.0.as_slice()) && same.binary == value.1 {
            return Ok(id);
        }
        // (memory)
        return Err(ApiError::modeled(
            "ResourceExistsException",
            "You can't modify an existing secret version, but you can create a new secret version with a different value.",
        ));
    }
    let stages: BTreeSet<String> = stages.map_or_else(
        || BTreeSet::from([CURRENT.to_owned()]),
        |s| s.into_iter().collect(),
    );
    let takes_current = stages.contains(CURRENT);
    let displaced = if takes_current {
        secret.staged(CURRENT).map(|(id, _)| id.to_owned())
    } else {
        None
    };
    for version in secret.versions.values_mut() {
        version.stages.retain(|s| !stages.contains(s));
    }
    if let Some(old) = displaced.as_deref() {
        for version in secret.versions.values_mut() {
            version.stages.remove(PREVIOUS);
        }
        if let Some(v) = secret.versions.get_mut(old) {
            v.stages.insert(PREVIOUS.to_owned());
        }
    }
    secret.versions.insert(
        id.clone(),
        Version {
            sealed,
            binary: value.1,
            stages,
            created_ms: now,
            accessed_day_ms: None,
        },
    );
    prune(secret, now);
    secret.changed_ms = now;
    Ok(id)
}

/// Drops the versions with no label that are a day old, and the oldest of those past the most a secret keeps.
fn prune(secret: &mut Secret, now: i64) {
    secret
        .versions
        .retain(|_, v| !v.stages.is_empty() || now - v.created_ms < DEPRECATED_KEPT_MS);
    while secret.versions.len() > MAX_VERSIONS {
        let oldest = secret
            .versions
            .iter()
            .filter(|(_, v)| v.stages.is_empty())
            .min_by_key(|(_, v)| v.created_ms)
            .map(|(id, _)| id.clone());
        match oldest {
            Some(id) => secret.versions.remove(&id),
            None => break,
        };
    }
}

impl Core {
    /// Whether a new secret may take `name`.
    fn name_is_free(&self, name: &str) -> Result<(), ApiError> {
        let Some(existing) = self.secrets.get(name) else {
            return Ok(());
        };
        Err(if existing.deleted_ms.is_some() {
            // (memory)
            invalid_request(
                "You can't create this secret because a secret with this name is already scheduled for deletion.",
            )
        } else {
            ApiError::modeled(
                "ResourceExistsException",
                format!("The operation failed because the secret {name} already exists."),
            )
        })
    }

    pub(crate) fn create_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let name = v.get(Create::NAME).unwrap_or_default();
        check_name(name)?;
        let value = value_of(v.get(Create::SECRET_STRING), v.get(Create::SECRET_BINARY))?;
        self.name_is_free(name)?;
        let tags: Vec<(String, String)> = v
            .get(Create::TAGS)
            .map(|t| {
                t.map(|t| {
                    (
                        t.get(Tag::KEY).unwrap_or_default().to_owned(),
                        t.get(Tag::VALUE).unwrap_or_default().to_owned(),
                    )
                })
                .collect()
            })
            .unwrap_or_default();
        if tags.len() > MAX_TAGS {
            return Err(invalid_parameter("The request has too many tags."));
        }
        let now = self.now_ms();
        let suffix =
            aws_seal::alphanumeric(6).ok_or_else(|| ApiError::internal("no randomness"))?;
        let mut secret = Secret {
            name: name.to_owned(),
            arn: format!(
                "arn:aws:secretsmanager:{}:{}:secret:{name}-{suffix}",
                call.caller.region, call.caller.account_id
            ),
            description: v.get(Create::DESCRIPTION).map(str::to_owned),
            kms_key_id: v.get(Create::KMS_KEY_ID).map(str::to_owned),
            kind: v.get(Create::TYPE).map(str::to_owned),
            tags: tags.into_iter().collect(),
            created_ms: now,
            changed_ms: now,
            accessed_day_ms: None,
            deleted_ms: None,
            purge_ms: None,
            versions: std::collections::BTreeMap::new(),
            policy: None,
        };
        let version = match &value {
            Some(value) => Some(add_version(
                &self.sealer,
                &mut secret,
                v.get(Create::CLIENT_REQUEST_TOKEN),
                value,
                None,
                now,
            )?),
            None => None,
        };
        let arn = secret.arn.clone();
        self.secrets.insert(name.to_owned(), secret);
        reply::<Created>(|o| {
            o.put(Created::ARN, &arn);
            o.put(Created::NAME, name);
            if let Some(id) = &version {
                o.put(Created::VERSION_ID, id);
            }
        })
    }

    pub(crate) fn put_secret_value(&mut self, call: &Call) -> Reply {
        let v = request::<Put>(call)?;
        let name = self.locate_live(v.get(Put::SECRET_ID).unwrap_or_default())?;
        let value = value_of(v.get(Put::SECRET_STRING), v.get(Put::SECRET_BINARY))?
            // (memory)
            .ok_or_else(|| {
                invalid_request("You must provide either SecretString or SecretBinary.")
            })?;
        let stages: Option<Vec<String>> = v
            .get(Put::VERSION_STAGES)
            .map(|s| s.map(str::to_owned).collect());
        let now = self.now_ms();
        let secret = self
            .secrets
            .get_mut(&name)
            .ok_or_else(crate::core::not_found)?;
        let id = add_version(
            &self.sealer,
            secret,
            v.get(Put::CLIENT_REQUEST_TOKEN),
            &value,
            stages,
            now,
        )?;
        let (arn, shown) = (
            secret.arn.clone(),
            secret
                .versions
                .get(&id)
                .map(|x| x.stages.clone())
                .unwrap_or_default(),
        );
        reply::<Put_>(|o| {
            o.put(Put_::ARN, &arn);
            o.put(Put_::NAME, &name);
            o.put(Put_::VERSION_ID, &id);
            o.put_list(Put_::VERSION_STAGES, |l| {
                for s in &shown {
                    l.push(s);
                }
            });
        })
    }

    pub(crate) fn update_secret(&mut self, call: &Call) -> Reply {
        let v = request::<Update>(call)?;
        let name = self.locate_live(v.get(Update::SECRET_ID).unwrap_or_default())?;
        let value = value_of(v.get(Update::SECRET_STRING), v.get(Update::SECRET_BINARY))?;
        let now = self.now_ms();
        let secret = self
            .secrets
            .get_mut(&name)
            .ok_or_else(crate::core::not_found)?;
        if let Some(d) = v.get(Update::DESCRIPTION) {
            secret.description = Some(d.to_owned());
        }
        if let Some(k) = v.get(Update::KMS_KEY_ID) {
            secret.kms_key_id = Some(k.to_owned());
        }
        if let Some(t) = v.get(Update::TYPE) {
            secret.kind = Some(t.to_owned());
        }
        secret.changed_ms = now;
        let version = match &value {
            Some(value) => Some(add_version(
                &self.sealer,
                secret,
                v.get(Update::CLIENT_REQUEST_TOKEN),
                value,
                None,
                now,
            )?),
            None => None,
        };
        let arn = secret.arn.clone();
        reply::<Updated>(|o| {
            o.put(Updated::ARN, &arn);
            o.put(Updated::NAME, &name);
            if let Some(id) = &version {
                o.put(Updated::VERSION_ID, id);
            }
        })
    }
}
