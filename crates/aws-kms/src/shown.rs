//! How a key is shown: its metadata, as `DescribeKey`, `CreateKey` and `ReplicateKey` answer it.

use aws_model_kms::keys::{KeyMetadata as Meta, MultiRegionConfiguration as Multi, MultiRegionKey};
use aws_schema::build::Object;

use crate::keys::seconds;
use crate::model::{Key, State};
use crate::multi::Set;
use crate::spec::Spec;

/// A key as `DescribeKey` and `CreateKey` show it.
pub(crate) fn metadata(
    m: &mut Object<'_, Meta>,
    key: &Key,
    account: &str,
    region: &str,
    set: Option<&Set>,
) {
    shown(m, key, account, region, set, false);
}

/// A new replica as `ReplicateKey` shows it. On AWS a replica is made in the background: the call answers `Creating`, not
/// enabled, with no current material yet (recorded: test_kms.py::test_multi_region_replica), and the key is `Enabled` moments
/// later. doze-aws makes it at once, so only this answer says `Creating`; `DescribeKey` already says `Enabled`.
pub(crate) fn creating(
    m: &mut Object<'_, Meta>,
    key: &Key,
    account: &str,
    region: &str,
    set: Option<&Set>,
) {
    shown(m, key, account, region, set, true);
}

fn shown(
    m: &mut Object<'_, Meta>,
    key: &Key,
    account: &str,
    region: &str,
    set: Option<&Set>,
    creating: bool,
) {
    m.put(Meta::AWS_ACCOUNT_ID, account);
    m.put(Meta::KEY_ID, &key.id);
    m.put(Meta::ARN, &key.arn);
    m.put_time(Meta::CREATION_DATE, seconds(key.created_ms));
    m.put_bool(Meta::ENABLED, !creating && key.state == State::Enabled);
    m.put(Meta::DESCRIPTION, &key.description);
    m.put(Meta::KEY_USAGE, key.usage.name());
    m.put(
        Meta::KEY_STATE,
        if creating {
            "Creating"
        } else {
            key.state.name()
        },
    );
    if let Some(at) = key.purge_ms {
        m.put_time(Meta::DELETION_DATE, seconds(at));
    }
    m.put(
        Meta::ORIGIN,
        if key.external { "EXTERNAL" } else { "AWS_KMS" },
    );
    if let Some(model) = &key.expiration_model {
        m.put(Meta::EXPIRATION_MODEL, model);
    }
    if let Some(at) = key.valid_to_ms {
        m.put_time(Meta::VALID_TO, seconds(at));
    }
    m.put(
        Meta::KEY_MANAGER,
        if key.aws_managed { "AWS" } else { "CUSTOMER" },
    );
    m.put(Meta::CUSTOMER_MASTER_KEY_SPEC, key.spec.name());
    m.put(Meta::KEY_SPEC, key.spec.name());
    algorithms(m, key);
    m.put_bool(Meta::MULTI_REGION, key.multi_region);
    if key.multi_region {
        let own = Set {
            primary: region.to_owned(),
            replicas: Vec::new(),
        };
        let set = set.unwrap_or(&own);
        m.put_object(Meta::MULTI_REGION_CONFIGURATION, |c| {
            multi_region(c, key, account, region, set);
        });
    }
    if !creating
        && key.spec == Spec::Symmetric
        && let Some(current) = key.current()
    {
        m.put(Meta::CURRENT_KEY_MATERIAL_ID, &current.id);
    }
}

/// The algorithms a key's spec and usage allow, each list only when it has any.
fn algorithms(m: &mut Object<'_, Meta>, key: &Key) {
    for (member, values) in [
        (
            Meta::ENCRYPTION_ALGORITHMS,
            key.spec.encryption_algorithms(key.usage),
        ),
        (
            Meta::SIGNING_ALGORITHMS,
            key.spec.signing_algorithms(key.usage),
        ),
        (Meta::MAC_ALGORITHMS, key.spec.mac_algorithms()),
        (
            Meta::KEY_AGREEMENT_ALGORITHMS,
            key.spec.key_agreement_algorithms(key.usage),
        ),
    ] {
        if !values.is_empty() {
            m.put_list(member, |l| {
                for v in values {
                    l.push(v);
                }
            });
        }
    }
}

/// A multi-Region key's set as `DescribeKey` shows it: which key is primary, and every replica.
fn multi_region(c: &mut Object<'_, Multi>, key: &Key, account: &str, region: &str, set: &Set) {
    let arn = |r: &str| format!("arn:aws:kms:{r}:{account}:key/{}", key.id);
    let kind = if set.primary == region {
        "PRIMARY"
    } else {
        "REPLICA"
    };
    c.put(Multi::MULTI_REGION_KEY_TYPE, kind);
    c.put_object(Multi::PRIMARY_KEY, |p: &mut Object<'_, MultiRegionKey>| {
        p.put(MultiRegionKey::ARN, &arn(&set.primary));
        p.put(MultiRegionKey::REGION, &set.primary);
    });
    c.put_list(Multi::REPLICA_KEYS, |l| {
        for r in &set.replicas {
            l.push(|p: &mut Object<'_, MultiRegionKey>| {
                p.put(MultiRegionKey::ARN, &arn(r));
                p.put(MultiRegionKey::REGION, r);
            });
        }
    });
}
