//! Reading values: `GetSecretValue`, `BatchGetSecretValue`.

use aws_model_secrets_manager::keys::{
    APIErrorType, BatchGetSecretValueRequest as Batch, BatchGetSecretValueResponse as Batched,
    GetSecretValueRequest as Get, GetSecretValueResponse as Got, SecretValueEntry as Entry,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, marked_for_deletion};
use crate::model::{CURRENT, Secret, Version, day_of};

/// What a read of one version shows.
struct Read {
    arn: String,
    name: String,
    id: String,
    value: Vec<u8>,
    binary: bool,
    stages: Vec<String>,
    created_ms: i64,
}

fn no_value(what: &str) -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        format!("Secrets Manager can't find the specified secret value for {what}"),
    )
}

#[allow(clippy::cast_precision_loss)]
fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

impl Core {
    /// Reads a version of a secret, and notes the day it was read.
    fn read(
        &mut self,
        id: &str,
        version: Option<&str>,
        stage: Option<&str>,
    ) -> Result<Read, ApiError> {
        let name = self.locate(id)?;
        let today = day_of(self.now_ms());
        let secret = self.secrets.get(&name).ok_or_else(crate::core::not_found)?;
        if secret.deleted_ms.is_some() {
            return Err(marked_for_deletion());
        }
        let (vid, v) = pick(secret, version, stage)?;
        let value = self
            .sealer
            .open(&v.sealed)
            .ok_or_else(|| ApiError::internal("a stored secret could not be opened"))?;
        let read = Read {
            arn: secret.arn.clone(),
            name: secret.name.clone(),
            id: vid.to_owned(),
            value,
            binary: v.binary,
            stages: v.stages.iter().cloned().collect(),
            created_ms: v.created_ms,
        };
        if let Some(secret) = self.secrets.get_mut(&name) {
            secret.accessed_day_ms = Some(today);
            if let Some(v) = secret.versions.get_mut(&read.id) {
                v.accessed_day_ms = Some(today);
            }
        }
        Ok(read)
    }

    pub(crate) fn get_secret_value(&mut self, call: &Call) -> Reply {
        let v = request::<Get>(call)?;
        let r = self.read(
            v.get(Get::SECRET_ID).unwrap_or_default(),
            v.get(Get::VERSION_ID),
            v.get(Get::VERSION_STAGE),
        )?;
        reply::<Got>(|o| {
            o.put(Got::ARN, &r.arn);
            o.put(Got::NAME, &r.name);
            o.put(Got::VERSION_ID, &r.id);
            if r.binary {
                o.put_blob(Got::SECRET_BINARY, &r.value);
            } else {
                o.put(Got::SECRET_STRING, &String::from_utf8_lossy(&r.value));
            }
            o.put_list(Got::VERSION_STAGES, |l| {
                for s in &r.stages {
                    l.push(s);
                }
            });
            o.put_time(Got::CREATED_DATE, seconds(r.created_ms));
        })
    }

    pub(crate) fn batch_get_secret_value(&mut self, call: &Call) -> Reply {
        let v = request::<Batch>(call)?;
        let ids: Vec<&str> = v
            .get(Batch::SECRET_ID_LIST)
            .map(Iterator::collect)
            .unwrap_or_default();
        if ids.is_empty() {
            return Err(crate::core::invalid_parameter(
                "Either SecretIdList or Filters must be provided.",
            ));
        }
        let mut found = Vec::new();
        let mut errors: Vec<(&str, ApiError)> = Vec::new();
        for id in ids {
            match self.read(id, None, None) {
                Ok(r) => found.push(r),
                Err(e) => errors.push((id, e)),
            }
        }
        reply::<Batched>(|o| {
            o.put_list(Batched::SECRET_VALUES, |l| {
                for r in &found {
                    l.push(|e: &mut Object<'_, Entry>| {
                        e.put(Entry::ARN, &r.arn);
                        e.put(Entry::NAME, &r.name);
                        e.put(Entry::VERSION_ID, &r.id);
                        if r.binary {
                            e.put_blob(Entry::SECRET_BINARY, &r.value);
                        } else {
                            e.put(Entry::SECRET_STRING, &String::from_utf8_lossy(&r.value));
                        }
                        e.put_list(Entry::VERSION_STAGES, |s| {
                            for x in &r.stages {
                                s.push(x);
                            }
                        });
                        e.put_time(Entry::CREATED_DATE, seconds(r.created_ms));
                    });
                }
            });
            if !errors.is_empty() {
                o.put_list(Batched::ERRORS, |l| {
                    for (id, e) in &errors {
                        l.push(|x: &mut Object<'_, APIErrorType>| {
                            x.put(APIErrorType::SECRET_ID, id);
                            x.put(
                                APIErrorType::ERROR_CODE,
                                e.shape.as_deref().unwrap_or(&e.code),
                            );
                            x.put(APIErrorType::MESSAGE, &e.message);
                        });
                    }
                });
            }
        })
    }
}

/// The version asked for: by id, by stage, or the current one. Asking for both an id and a stage wants a version that is
/// both.
fn pick<'a>(
    secret: &'a Secret,
    version: Option<&str>,
    stage: Option<&str>,
) -> Result<(&'a str, &'a Version), ApiError> {
    if let Some(id) = version {
        let (key, v) = secret
            .versions
            .get_key_value(id)
            .ok_or_else(|| no_value(&format!("VersionId: {id}")))?;
        if stage.is_some_and(|s| !v.stages.contains(s)) {
            return Err(no_value(&format!(
                "staging label: {}",
                stage.unwrap_or_default()
            )));
        }
        return Ok((key.as_str(), v));
    }
    let wanted = stage.unwrap_or(CURRENT);
    secret
        .staged(wanted)
        .ok_or_else(|| no_value(&format!("staging label: {wanted}")))
}
