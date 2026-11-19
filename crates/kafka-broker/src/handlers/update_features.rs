//! UpdateFeatures (API 57, KIP-584): change the level a feature is finalized at. What `kafka-features.sh upgrade`,
//! `downgrade` and `disable`, and the admin clients' `updateFeatures`, send.
//!
//! Each update is checked as Kafka 4.4.0's controller checks it (`QuorumController.updateFeatures`,
//! `FeatureControlManager.updateFeature`), in its order, against the features the broker's Kafka major supports
//! (`Profile::features`): an upgrade type it does not know, a level below 0, a level outside what is supported, a downgrade
//! asked as an upgrade or an upgrade as a downgrade, a downgrade of `metadata.version` (which on 4.4.0's latest level always
//! changes metadata, so Kafka refuses it too), a downgrade of `kraft.version`, and a feature's dependencies. The first that
//! fails fails the whole request, with Kafka's text.
//!
//! The levels themselves are this broker's Kafka major's and do not move (`docs/port/behaviours/admin.md`
//! ADM-D9): a request that passes every check and changes no level, or only validates, succeeds; one that would change a level
//! is refused with INVALID_UPDATE_VERSION and says why. Kafka would apply it.

use std::collections::BTreeMap;

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::update_features_request::FeatureUpdateKey;
use kafka_wire::messages::update_features_response::UpdatableFeatureResult;
use kafka_wire::messages::{UpdateFeaturesRequest, UpdateFeaturesResponse};
use kafka_wire::profile::{FeatureLevel, Profile};

use crate::{Broker, Caller};

/// `FeatureUpdate.UpgradeType`: UPGRADE 1, SAFE_DOWNGRADE 2, UNSAFE_DOWNGRADE 3.
const UPGRADE: i8 = 1;
const SAFE_DOWNGRADE: i8 = 2;
const UNSAFE_DOWNGRADE: i8 = 3;

/// The features of Kafka 4.4.0's `Feature` enum other than `metadata.version` (its unit-test ones are matched by prefix).
const KNOWN: &[&str] = &[
    "kraft.version",
    "transaction.version",
    "group.version",
    "eligible.leader.replicas.version",
    "share.version",
    "streams.version",
    "test.feature.version",
];

/// `eligible.leader.replicas.version` 1 needs `metadata.version` 23 (4.0-IV1); no other feature of 4.4.0 depends on
/// another (`EligibleLeaderReplicasVersion.ELRV_1`).
const DEPENDENCIES: &[(&str, i16, &str, i16)] = &[(
    "eligible.leader.replicas.version",
    1,
    "metadata.version",
    23,
)];

impl<F: Fs> Broker<F> {
    /// Checks every update, and applies none (see the module's text). The caller needs ALTER on the cluster; without it the
    /// answer is CLUSTER_AUTHORIZATION_FAILED with the request's text and no results (`UpdateFeaturesRequest.getErrorResponse`).
    pub(crate) fn update_features(
        &self,
        caller: &Caller,
        req: &UpdateFeaturesRequest,
        version: i16,
    ) -> UpdateFeaturesResponse {
        let mut resp = UpdateFeaturesResponse::default();
        if !self.cluster_allows(caller, Operation::Alter) {
            resp.error_code = ResponseError::ClusterAuthorizationFailed.code();
            resp.error_message = Some(StrBytes::from_string(caller.cluster_denied("ALTER")));
            return resp;
        }
        let supported = Profile::new(self.config.kafka_major).features();
        let node = self.config.node_id;
        let mut proposed: BTreeMap<&str, i16> =
            supported.iter().map(|f| (f.name, f.level)).collect();
        for u in &req.feature_updates {
            proposed.insert(u.feature.as_str(), u.max_version_level);
        }
        let mut changes = false;
        for u in java_hash_map_order(&req.feature_updates) {
            let name = u.feature.as_str();
            let level = u.max_version_level;
            let current = supported
                .iter()
                .find(|f| f.name == name)
                .map_or(0, |f| f.level);
            if let Err(why) = check(
                &supported,
                node,
                name,
                level,
                u.upgrade_type,
                current,
                &proposed,
            ) {
                resp.error_code = ResponseError::InvalidUpdateVersion.code();
                resp.error_message = Some(StrBytes::from_string(format!(
                    "The update failed for all features since the following feature had an error: {why}"
                )));
                return resp;
            }
            changes |= level != current;
        }
        if changes && !req.validate_only {
            resp.error_code = ResponseError::InvalidUpdateVersion.code();
            resp.error_message = Some(StrBytes::from_string(format!(
                "The update failed for all features since the following feature had an error: this broker keeps \
                 the feature levels of Kafka {} as they are; they change with --kafka-version",
                self.config.kafka_major.get()
            )));
            return resp;
        }
        resp.error_message = None;
        // Results only before v2, and only on success (`QuorumController.updateFeatures`).
        if version <= 1 {
            resp.results = (req.feature_updates.iter())
                .map(|u| {
                    let mut r = UpdatableFeatureResult::default();
                    r.feature = u.feature.clone();
                    // `Errors.NONE.message()`, which is the name.
                    r.error_message = Some(StrBytes::from_static_str("NONE"));
                    r
                })
                .collect();
        }
        resp
    }
}

/// The updates in the order Kafka checks them: its `HashMap<String, Short>` of them, one per name (the last of a name wins,
/// in the first one's place), iterated by bucket (`String.hashCode` spread as `h ^ (h >>> 16)`, over a table of 16
/// doubled whenever it is more than three-quarters full), and within a bucket in the order put.
fn java_hash_map_order(updates: &[FeatureUpdateKey]) -> Vec<&FeatureUpdateKey> {
    let mut unique: Vec<&FeatureUpdateKey> = Vec::new();
    for u in updates {
        match unique.iter_mut().find(|k| k.feature == u.feature) {
            Some(k) => *k = u,
            None => unique.push(u),
        }
    }
    let mut buckets = 16usize;
    while unique.len() * 4 > buckets * 3 {
        buckets *= 2;
    }
    let hash = |name: &str| {
        let h = name
            .encode_utf16()
            .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(u32::from(c)));
        usize::try_from(h ^ (h >> 16)).unwrap_or(0) & (buckets - 1)
    };
    unique.sort_by_key(|u| hash(u.feature.as_str()));
    unique
}

/// `FeatureControlManager.updateFeature` for one update: the text of the first check it fails.
fn check(
    supported: &[FeatureLevel],
    node: i32,
    name: &str,
    level: i16,
    upgrade_type: i8,
    current: i16,
    proposed: &BTreeMap<&str, i16>,
) -> Result<(), String> {
    let invalid = |why: &str| format!("Invalid update version {level} for feature {name}. {why}");
    if ![UPGRADE, SAFE_DOWNGRADE, UNSAFE_DOWNGRADE].contains(&upgrade_type) {
        return Err(invalid(
            "The controller does not support the given upgrade type.",
        ));
    }
    if level < 0 {
        return Err(invalid("A feature version cannot be less than 0."));
    }
    let (min, max) = supported
        .iter()
        .find(|f| f.name == name)
        .map_or((0, 0), |f| (f.min, f.max));
    if !(min..=max).contains(&level) {
        // `QuorumFeatures.reasonNotSupported`: a feature not supported at all is the range 0 to 0.
        let why = if max == 0 {
            format!("Local controller {node} does not support this feature.")
        } else {
            format!("Local controller {node} only supports versions {min}-{max}")
        };
        return Err(invalid(&why));
    }
    if level < current && upgrade_type == UPGRADE {
        return Err(invalid(
            "Can't downgrade the version of this feature without setting the upgrade type to either safe or unsafe \
             downgrade.",
        ));
    }
    if level > current && upgrade_type != UPGRADE {
        return Err(invalid("Can't downgrade to a newer version."));
    }
    match name {
        "metadata.version" if level < current => {
            let why = if upgrade_type == UNSAFE_DOWNGRADE {
                "Unsafe metadata downgrade is not supported in this version."
            } else {
                "Refusing to perform the requested downgrade because it might delete metadata information."
            };
            Err(format!(
                "Unsupported metadata.version downgrade from {current} to {level}. {why}"
            ))
        }
        "metadata.version" => Ok(()),
        "kraft.version" if upgrade_type != UPGRADE && level != current => {
            Err(invalid("Can't downgrade the version of this feature."))
        }
        "kraft.version" => Ok(()),
        _ => {
            // `Feature.featureFromName`: a name Kafka 4.4.0 has no feature of (only level 0 of one gets this far).
            if !KNOWN.contains(&name) && !name.starts_with("unit.test.feature.version.") {
                return Err(invalid(&format!("Feature {name} not found.")));
            }
            for &(feature, at, needs, needed) in DEPENDENCIES {
                if feature == name && level >= at && proposed.get(needs).is_none_or(|&l| l < needed)
                {
                    return Err(invalid(&format!(
                        "{name} could not be set to {level} because it depends on {needs} level {needed}"
                    )));
                }
            }
            Ok(())
        }
    }
}
