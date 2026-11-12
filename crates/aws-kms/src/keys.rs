//! Making keys, looking at them, and moving them between states: `CreateKey`, `DescribeKey`, `ListKeys`, `EnableKey`,
//! `DisableKey`, `ScheduleKeyDeletion`, `CancelKeyDeletion`, `UpdateKeyDescription`.

use std::collections::BTreeMap;

use aws_model_kms::keys::{
    CancelKeyDeletionRequest as Cancel, CancelKeyDeletionResponse as Cancelled,
    CreateKeyRequest as Create, CreateKeyResponse as Created, DescribeKeyRequest as Describe,
    DescribeKeyResponse as Described, DisableKeyRequest as Disable, EnableKeyRequest as Enable,
    KeyListEntry, KeyMetadata as Meta, ListKeysRequest as List, ListKeysResponse as Listed,
    MultiRegionConfiguration as Multi, MultiRegionKey, ScheduleKeyDeletionRequest as Schedule,
    ScheduleKeyDeletionResponse as Scheduled, Tag, UpdateKeyDescriptionRequest as Describe_,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::crypto::material_id;
use crate::find::key_arn;
use crate::model::{DAY_MS, Key, Material, State};
use crate::spec::{Spec, Usage};

const DEFAULT_WINDOW_DAYS: i64 = 30;
const LIST_PAGE: usize = 100;

#[allow(clippy::cast_precision_loss)]
pub(crate) fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

pub(crate) fn invalid_state(message: String) -> ApiError {
    ApiError::modeled("KMSInvalidStateException", message)
}

/// The key policy a key gets when it is made without one.
pub(crate) fn default_policy(account: &str) -> String {
    format!(
        r#"{{"Version":"2012-10-17","Id":"key-default-1","Statement":[{{"Sid":"Enable IAM User Permissions","Effect":"Allow","Principal":{{"AWS":"arn:aws:iam::{account}:root"}},"Action":"kms:*","Resource":"*"}}]}}"#
    )
}

/// A key as `DescribeKey` and `CreateKey` show it.
pub(crate) fn metadata(m: &mut Object<'_, Meta>, key: &Key, account: &str, region: &str) {
    m.put(Meta::AWS_ACCOUNT_ID, account);
    m.put(Meta::KEY_ID, &key.id);
    m.put(Meta::ARN, &key.arn);
    m.put_time(Meta::CREATION_DATE, seconds(key.created_ms));
    m.put_bool(Meta::ENABLED, key.state == State::Enabled);
    m.put(Meta::DESCRIPTION, &key.description);
    m.put(Meta::KEY_USAGE, key.usage.name());
    m.put(Meta::KEY_STATE, key.state.name());
    if let Some(at) = key.purge_ms {
        m.put_time(Meta::DELETION_DATE, seconds(at));
    }
    m.put(Meta::ORIGIN, "AWS_KMS");
    m.put(Meta::KEY_MANAGER, "CUSTOMER");
    m.put(Meta::CUSTOMER_MASTER_KEY_SPEC, key.spec.name());
    m.put(Meta::KEY_SPEC, key.spec.name());
    for (member, values) in [
        (
            Meta::ENCRYPTION_ALGORITHMS,
            key.spec.encryption_algorithms(),
        ),
        (Meta::SIGNING_ALGORITHMS, key.spec.signing_algorithms()),
        (Meta::MAC_ALGORITHMS, key.spec.mac_algorithms()),
    ] {
        if !values.is_empty() {
            m.put_list(member, |l| {
                for v in values {
                    l.push(v);
                }
            });
        }
    }
    m.put_bool(Meta::MULTI_REGION, key.multi_region);
    if key.multi_region {
        m.put_object(
            Meta::MULTI_REGION_CONFIGURATION,
            |c: &mut Object<'_, Multi>| {
                c.put(Multi::MULTI_REGION_KEY_TYPE, "PRIMARY");
                c.put_object(Multi::PRIMARY_KEY, |p: &mut Object<'_, MultiRegionKey>| {
                    p.put(MultiRegionKey::ARN, &key.arn);
                    p.put(MultiRegionKey::REGION, region);
                });
                c.put_list(Multi::REPLICA_KEYS, |_| {});
            },
        );
    }
    if key.spec == Spec::Symmetric
        && let Some(current) = key.current()
    {
        m.put(Meta::CURRENT_KEY_MATERIAL_ID, &current.id);
    }
}

/// The spec and the usage a `CreateKey` asks for, held to each other.
fn spec_and_usage(v: &aws_schema::view::View<'_, Create>) -> Result<(Spec, Usage), ApiError> {
    let spec = match v
        .get(Create::KEY_SPEC)
        .or(v.get(Create::CUSTOMER_MASTER_KEY_SPEC))
    {
        None => Spec::Symmetric,
        Some(name) => Spec::parse(name).ok_or_else(|| {
            ApiError::modeled(
                "UnsupportedOperationException",
                format!("The key spec {name} is not built in doze-aws."),
            )
        })?,
    };
    let usage = match v.get(Create::KEY_USAGE) {
        Some(name) => Usage::parse(name).ok_or_else(|| validation(format!("KeyUsage {name} is not supported.")))?,
        // (memory)
        None => spec.default_usage().ok_or_else(|| {
            validation("You must specify a KeyUsage value for all KMS keys except for symmetric encryption keys.")
        })?,
    };
    if !spec.allows(usage) {
        return Err(validation(format!(
            "KeyUsage {} is not compatible with KeySpec {}",
            usage.name(),
            spec.name()
        )));
    }
    if v.get(Create::ORIGIN).is_some_and(|o| o != "AWS_KMS")
        || v.get(Create::CUSTOM_KEY_STORE_ID).is_some()
    {
        return Err(ApiError::modeled(
            "UnsupportedOperationException",
            "Only keys made by KMS are built in doze-aws: no imported key material and no custom key stores.",
        ));
    }
    Ok((spec, usage))
}

impl Core {
    pub(crate) fn create_key(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let (spec, usage) = spec_and_usage(&v)?;
        let multi_region = v.get(Create::MULTI_REGION).unwrap_or(false);
        let id = if multi_region {
            format!("mrk-{}", id_hex()?)
        } else {
            aws_seal::uuid_v4().ok_or_else(no_randomness)?
        };
        let tags: BTreeMap<String, String> = v
            .get(Create::TAGS)
            .into_iter()
            .flatten()
            .map(|t| {
                (
                    t.get(Tag::TAG_KEY).unwrap_or_default().to_owned(),
                    t.get(Tag::TAG_VALUE).unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let now = self.now_ms();
        let key = Key {
            arn: key_arn(call, &id),
            id: id.clone(),
            spec,
            usage,
            description: v.get(Create::DESCRIPTION).unwrap_or_default().to_owned(),
            state: State::Enabled,
            created_ms: now,
            purge_ms: None,
            window_days: None,
            materials: vec![self.new_material(spec, now)?],
            policy: Some(
                v.get(Create::POLICY)
                    .map_or_else(|| default_policy(&call.caller.account_id), str::to_owned),
            ),
            tags,
            rotation_enabled: false,
            rotation_days: 365,
            next_rotation_ms: None,
            multi_region,
            grants: Vec::new(),
        };
        let shown = key.clone();
        self.keys.insert(id, key);
        reply::<Created>(|o| {
            o.put_object(Created::KEY_METADATA, |m| {
                metadata(m, &shown, &call.caller.account_id, &call.caller.region)
            });
        })
    }

    /// A new generation of material for a key of `spec`, sealed.
    pub(crate) fn new_material(&self, spec: Spec, now: i64) -> Result<Material, ApiError> {
        let raw = crate::asym::generate(spec).ok_or_else(no_randomness)?;
        Ok(Material {
            id: aws_core::hex::encode(&material_id(&raw)),
            sealed: self.sealer.seal(&raw).ok_or_else(no_randomness)?,
            created_ms: now,
            retired_by: None,
            retired_ms: None,
        })
    }

    pub(crate) fn describe_key(&mut self, call: &Call) -> Reply {
        let v = request::<Describe>(call)?;
        let key = self.key(call, v.get(Describe::KEY_ID).unwrap_or_default())?;
        reply::<Described>(|o| {
            o.put_object(Described::KEY_METADATA, |m| {
                metadata(m, key, &call.caller.account_id, &call.caller.region)
            });
        })
    }

    pub(crate) fn list_keys(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::LIMIT)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let after = v.get(List::MARKER).unwrap_or_default();
        let mut rows: Vec<&Key> = self
            .keys
            .iter()
            .map(|(_, k)| k)
            .filter(|k| k.id.as_str() > after)
            .collect();
        let more = rows.len() > limit;
        rows.truncate(limit);
        let marker = more.then(|| rows.last().map(|k| k.id.clone())).flatten();
        reply::<Listed>(|o| {
            o.put_list(Listed::KEYS, |l| {
                for k in &rows {
                    l.push(|e: &mut Object<'_, KeyListEntry>| {
                        e.put(KeyListEntry::KEY_ID, &k.id);
                        e.put(KeyListEntry::KEY_ARN, &k.arn);
                    });
                }
            });
            o.put_bool(Listed::TRUNCATED, more);
            if let Some(m) = &marker {
                o.put(Listed::NEXT_MARKER, m);
            }
        })
    }

    /// Moves a key that is not pending deletion to `to`.
    fn set_state(&mut self, call: &Call, given: &str, to: State) -> Reply {
        let id = self.locate(call, given)?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| crate::find::not_found_key(call, &id))?;
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        key.state = to;
        empty()
    }

    pub(crate) fn enable_key(&mut self, call: &Call) -> Reply {
        let v = request::<Enable>(call)?;
        self.set_state(
            call,
            v.get(Enable::KEY_ID).unwrap_or_default(),
            State::Enabled,
        )
    }

    pub(crate) fn disable_key(&mut self, call: &Call) -> Reply {
        let v = request::<Disable>(call)?;
        self.set_state(
            call,
            v.get(Disable::KEY_ID).unwrap_or_default(),
            State::Disabled,
        )
    }

    pub(crate) fn schedule_key_deletion(&mut self, call: &Call) -> Reply {
        let v = request::<Schedule>(call)?;
        let days = v
            .get(Schedule::PENDING_WINDOW_IN_DAYS)
            .unwrap_or(DEFAULT_WINDOW_DAYS);
        if !(7..=30).contains(&days) {
            return Err(validation("PendingWindowInDays must be between 7 and 30"));
        }
        let id = self.locate(call, v.get(Schedule::KEY_ID).unwrap_or_default())?;
        let at = self.now_ms() + days * DAY_MS;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| crate::find::not_found_key(call, &id))?;
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        key.state = State::PendingDeletion;
        key.purge_ms = Some(at);
        key.window_days = Some(days);
        let arn = key.arn.clone();
        self.purge_at(at);
        reply::<Scheduled>(|o| {
            o.put(Scheduled::KEY_ID, &arn);
            o.put_time(Scheduled::DELETION_DATE, seconds(at));
            o.put(Scheduled::KEY_STATE, "PendingDeletion");
            o.put_int(Scheduled::PENDING_WINDOW_IN_DAYS, days);
        })
    }

    pub(crate) fn cancel_key_deletion(&mut self, call: &Call) -> Reply {
        let v = request::<Cancel>(call)?;
        let id = self.locate(call, v.get(Cancel::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| crate::find::not_found_key(call, &id))?;
        if key.state != State::PendingDeletion {
            // (memory)
            return Err(invalid_state(format!(
                "{} is not pending deletion.",
                key.arn
            )));
        }
        // Recorded 2026-11-09: a key whose deletion is cancelled comes back Disabled, not Enabled.
        key.state = State::Disabled;
        key.purge_ms = None;
        key.window_days = None;
        let arn = key.arn.clone();
        reply::<Cancelled>(|o| {
            o.put(Cancelled::KEY_ID, &arn);
        })
    }

    pub(crate) fn update_key_description(&mut self, call: &Call) -> Reply {
        let v = request::<Describe_>(call)?;
        let id = self.locate(call, v.get(Describe_::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| crate::find::not_found_key(call, &id))?;
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        key.description = v.get(Describe_::DESCRIPTION).unwrap_or_default().to_owned();
        empty()
    }
}

pub(crate) fn no_randomness() -> ApiError {
    ApiError::internal("no randomness")
}

fn id_hex() -> Result<String, ApiError> {
    Ok(aws_core::hex::encode(
        &aws_seal::random_bytes::<16>().ok_or_else(no_randomness)?,
    ))
}
