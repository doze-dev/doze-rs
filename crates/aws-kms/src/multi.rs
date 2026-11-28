//! Multi-Region keys: `ReplicateKey` and `UpdatePrimaryRegion`. A key made with `MultiRegion=true` (an `mrk-` id) is the primary of
//! a set; `ReplicateKey` makes a replica in another region with the same id and the same material, so what one seals the other
//! opens (the blob names the key id and the material, not the region). Each region's actor keeps its own keys; the set, which
//! region is primary and which hold replicas, is kept beside the key in every region of the set (`multiregion` table: the key
//! record keeps the shape it had).
//!
//! Work across regions is done by the service outside the actors (`Kms::ask`), from a [`Fanout`] the actor of the calling region
//! answers with: the replica is made in its region first (its answer is the call's), then the set's new shape is written in each
//! region, then new material (a rotation of the primary) goes to each replica. No actor waits on another.
//!
//! Shared, as on AWS: the id, the spec and usage, the material (a rotation of the primary reaches every replica) and the rotation
//! setting. Not shared: description, policy, tags, enabled state, aliases. Departures (`docs/port/aws/stages/ops1.md`): the primary
//! region changes at once, not after an `Updating` state; a primary with replicas cannot be scheduled for deletion here (AWS puts
//! it in `PendingReplicaDeletion` until the replicas are gone): delete the replicas first.

use std::collections::BTreeMap;

use aws_model_kms::keys::{
    ReplicateKeyRequest as Replicate, ReplicateKeyResponse as Replicated, Tag,
    UpdatePrimaryRegionRequest as UpdatePrimary,
};
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};
use serde::{Deserialize, Serialize};

use crate::core::{Answer, Ask, Core};
use crate::keys::{default_policy, invalid_state};
use crate::model::{Key, Material, State};
use crate::shown::creating;

/// A multi-Region key's set, as each region of it keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Set {
    pub(crate) primary: String,
    pub(crate) replicas: Vec<String>,
}

impl Set {
    /// Every region of the set, the primary first.
    pub(crate) fn regions(&self) -> Vec<String> {
        std::iter::once(self.primary.clone())
            .chain(self.replicas.iter().cloned())
            .collect()
    }
}

/// A generation of material, open, on its way to another region (which seals it under its own sealing key).
#[derive(Clone, Debug)]
pub(crate) struct Open {
    pub(crate) id: String,
    pub(crate) plain: Vec<u8>,
    pub(crate) created_ms: i64,
    pub(crate) retired_by: Option<String>,
    pub(crate) retired_ms: Option<i64>,
}

/// What a replica is made from.
#[derive(Clone, Debug)]
pub(crate) struct Seed {
    pub(crate) key: Key,
    pub(crate) materials: Vec<Open>,
    pub(crate) account: String,
    pub(crate) set: Set,
}

/// The work a call leaves for other regions.
#[derive(Debug, Default)]
pub(crate) struct Fanout {
    /// A replica to make in a region; its answer is the call's.
    pub(crate) create: Option<(String, Box<Seed>)>,
    /// The set's new shape, for each region named.
    pub(crate) set: Option<(String, Set, Vec<String>)>,
    /// A key's material now, for each replica named.
    pub(crate) materials: Option<(String, Vec<Open>, Vec<String>)>,
}

fn unsupported(message: impl Into<String>) -> ApiError {
    ApiError::modeled("UnsupportedOperationException", message)
}

/// The region of a key's ARN.
pub(crate) fn region_of(arn: &str) -> &str {
    arn.split(':').nth(3).unwrap_or_default()
}

impl Core {
    /// The set of a multi-Region key (a key with no record is the primary of a set of one).
    pub(crate) fn set_of(&self, key: &Key) -> Option<Set> {
        key.multi_region.then(|| {
            self.sets.get(&key.id).cloned().unwrap_or_else(|| Set {
                primary: region_of(&key.arn).to_owned(),
                replicas: Vec::new(),
            })
        })
    }

    /// Whether `key` is the replica of a multi-Region key.
    pub(crate) fn is_replica(&self, key: &Key) -> bool {
        self.set_of(key)
            .is_some_and(|s| s.primary != region_of(&key.arn))
    }

    /// The material of `key`, open, every generation.
    pub(crate) fn open_materials(&self, key: &Key) -> Result<Vec<Open>, ApiError> {
        key.materials
            .iter()
            .map(|m| {
                let plain = self
                    .sealer
                    .open(&m.sealed)
                    .ok_or_else(|| ApiError::internal("the key material could not be opened"))?;
                Ok(Open {
                    id: m.id.clone(),
                    plain,
                    created_ms: m.created_ms,
                    retired_by: m.retired_by.clone(),
                    retired_ms: m.retired_ms,
                })
            })
            .collect()
    }

    /// The primary key of a set, held to what `operation` needs of it.
    fn primary(&self, call: &Call, given: &str) -> Result<(Key, Set), ApiError> {
        let key = self.key(call, given)?;
        let set = self.set_of(key).ok_or_else(|| {
            // Recorded (test_kms.py::test_multi_region_replica).
            unsupported(format!("{} is not a multi-region key.", key.arn))
        })?;
        if set.primary != call.caller.region {
            // Recorded (test_kms.py::test_multi_region_replica).
            return Err(unsupported(format!(
                "{} is not a multi-region primary key.",
                key.arn
            )));
        }
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        Ok((key.clone(), set))
    }

    pub(crate) fn replicate_key(&mut self, call: &Call) -> Reply {
        let v = request::<Replicate>(call)?;
        let given = v.get(Replicate::KEY_ID).unwrap_or_default();
        let region = v
            .get(Replicate::REPLICA_REGION)
            .unwrap_or_default()
            .to_owned();
        self.ensure_material(call, given)?;
        let (key, mut set) = self.primary(call, given)?;
        if key.external {
            return Err(unsupported(
                "doze-aws does not replicate a multi-Region key with imported material: make the primary with Origin AWS_KMS.",
            ));
        }
        if set.regions().contains(&region) {
            // Recorded (test_kms.py::test_multi_region_replica), for a region that holds a replica; the primary's own region (memory).
            return Err(ApiError::modeled(
                "AlreadyExistsException",
                "You can create only one replica of each primary key in each AWS Region.",
            ));
        }
        set.replicas.push(region.clone());
        let tags: BTreeMap<String, String> = v
            .get(Replicate::TAGS)
            .into_iter()
            .flatten()
            .map(|t| {
                let k = t.get(Tag::TAG_KEY).unwrap_or_default().to_owned();
                (k, t.get(Tag::TAG_VALUE).unwrap_or_default().to_owned())
            })
            .collect();
        let account = call.caller.account_id.clone();
        let mut replica = key.clone();
        replica.arn = format!("arn:aws:kms:{region}:{account}:key/{}", key.id);
        replica.description = v.get(Replicate::DESCRIPTION).unwrap_or_default().to_owned();
        replica.policy = Some(
            v.get(Replicate::POLICY)
                .map_or_else(|| default_policy(&account), str::to_owned),
        );
        replica.tags = tags;
        replica.state = State::Enabled;
        replica.created_ms = self.now_ms();
        replica.grants = Vec::new();
        let seed = Seed {
            materials: self.open_materials(&key)?,
            key: replica,
            account,
            set: set.clone(),
        };
        self.fanout = Some(Fanout {
            create: Some((region, Box::new(seed))),
            set: Some((key.id.clone(), set.clone(), set.regions())),
            materials: None,
        });
        // The replica's region answers the call.
        empty()
    }

    pub(crate) fn update_primary_region(&mut self, call: &Call) -> Reply {
        let v = request::<UpdatePrimary>(call)?;
        let (key, set) = self.primary(call, v.get(UpdatePrimary::KEY_ID).unwrap_or_default())?;
        let next = v.get(UpdatePrimary::PRIMARY_REGION).unwrap_or_default();
        if !set.replicas.iter().any(|r| r == next) {
            // Recorded (test_kms.py::test_update_primary_region): AWS names the key by its id.
            return Err(crate::core::validation(format!(
                "{} does not have a replica key in the specified Region.",
                key.id
            )));
        }
        let mut replicas: Vec<String> = set
            .replicas
            .iter()
            .filter(|r| *r != next)
            .cloned()
            .collect();
        replicas.push(set.primary.clone());
        let new = Set {
            primary: next.to_owned(),
            replicas,
        };
        self.fanout = Some(Fanout {
            set: Some((key.id.clone(), new.clone(), new.regions())),
            ..Fanout::default()
        });
        empty()
    }

    /// Makes the replica a seed describes; answers as `ReplicateKey`.
    pub(crate) fn make_replica(&mut self, seed: Seed) -> Reply {
        let Seed {
            mut key,
            materials,
            account,
            set,
        } = seed;
        if self.keys.get(&key.id).is_some() {
            // (memory)
            return Err(ApiError::modeled(
                "AlreadyExistsException",
                format!("{} already exists.", key.arn),
            ));
        }
        key.materials = self.seal_materials(materials)?;
        self.sets.insert(key.id.clone(), set.clone());
        self.keys.insert(key.id.clone(), key.clone());
        let region = region_of(&key.arn).to_owned();
        reply::<Replicated>(|o| {
            o.put_object(Replicated::REPLICA_KEY_METADATA, |m| {
                creating(m, &key, &account, &region, Some(&set));
            });
            if let Some(p) = &key.policy {
                o.put(Replicated::REPLICA_POLICY, p);
            }
            o.put_list(Replicated::REPLICA_TAGS, |l| {
                for (k, v) in &key.tags {
                    l.push(|t| {
                        t.put(Tag::TAG_KEY, k);
                        t.put(Tag::TAG_VALUE, v);
                    });
                }
            });
        })
    }

    fn seal_materials(&self, materials: Vec<Open>) -> Result<Vec<Material>, ApiError> {
        materials
            .into_iter()
            .map(|m| {
                let sealed = if m.plain.is_empty() {
                    Vec::new()
                } else {
                    self.sealer
                        .seal(&m.plain)
                        .ok_or_else(|| ApiError::internal("no randomness"))?
                };
                Ok(Material {
                    id: m.id,
                    sealed,
                    created_ms: m.created_ms,
                    retired_by: m.retired_by,
                    retired_ms: m.retired_ms,
                })
            })
            .collect()
    }

    /// Writes the set's new shape for `id`, if this region has the key.
    pub(crate) fn take_set(&mut self, id: &str, set: Set) {
        if self.keys.get(id).is_some() {
            self.sets.insert(id.to_owned(), set);
        }
    }

    /// Takes the primary's material, if this region has the key.
    pub(crate) fn take_materials(&mut self, id: &str, materials: Vec<Open>) {
        if self.keys.get(id).is_none() {
            return;
        }
        if let Ok(sealed) = self.seal_materials(materials)
            && let Some(key) = self.keys.get_mut(id)
        {
            key.materials = sealed;
        }
    }

    /// After a rotation of `id`: its new material goes to its replicas.
    pub(crate) fn spread_materials(&mut self, id: &str) -> Result<(), ApiError> {
        let Some(key) = self.keys.get(id) else {
            return Ok(());
        };
        let Some(set) = self.set_of(key).filter(|s| !s.replicas.is_empty()) else {
            return Ok(());
        };
        let materials = self.open_materials(key)?;
        self.fanout = Some(Fanout {
            materials: Some((id.to_owned(), materials, set.replicas)),
            ..Fanout::default()
        });
        Ok(())
    }

    /// Rotation is a shared property of a multi-Region key: it is set, and done, on the primary. The words are recorded for
    /// `RotateKeyOnDemand` (test_kms.py::test_multi_region_replica) and follow the same form for the other operations (memory).
    pub(crate) fn not_on_replica(&self, id: &str, operation: &str) -> Result<(), ApiError> {
        match self.keys.get(id) {
            Some(key) if self.is_replica(key) => Err(unsupported(format!(
                "{} is a multi-region replica key which is not valid for {operation}.",
                key.arn
            ))),
            _ => Ok(()),
        }
    }

    /// A primary key with replicas is not scheduled for deletion here (see the module's departures).
    pub(crate) fn before_deletion(&self, call: &Call, id: &str) -> Result<(), ApiError> {
        let Some(key) = self.keys.get(id) else {
            return Ok(());
        };
        match self.set_of(key) {
            Some(set) if set.primary == call.caller.region && !set.replicas.is_empty() => {
                Err(unsupported(format!(
                    "{} is a multi-Region primary key with replicas in {}. doze-aws does not keep a primary in PendingReplicaDeletion: schedule its replicas for deletion first, or make another Region primary (UpdatePrimaryRegion).",
                    key.arn,
                    set.replicas.join(", ")
                )))
            }
            _ => Ok(()),
        }
    }

    /// A replica scheduled for deletion leaves its set (`present`: false), and one whose deletion is cancelled comes back.
    pub(crate) fn replica_membership(&mut self, id: &str, region: &str, present: bool) {
        let Some(key) = self.keys.get(id) else {
            return;
        };
        let Some(mut set) = self.set_of(key).filter(|s| s.primary != region) else {
            return;
        };
        set.replicas.retain(|r| r != region);
        if present {
            set.replicas.push(region.to_owned());
        }
        let mut regions = set.regions();
        regions.retain(|r| r != region);
        self.fanout = Some(Fanout {
            set: Some((id.to_owned(), set, regions)),
            ..Fanout::default()
        });
    }
}

impl Core {
    /// Work another region of a multi-Region key hands this one.
    pub(crate) fn region_work(&mut self, ask: Ask) -> Answer {
        let mut reply = aws_wire::handler::empty();
        match ask {
            Ask::Replica(seed) => reply = self.make_replica(*seed),
            Ask::Set(id, set) => self.take_set(&id, set),
            Ask::Materials(id, materials) => self.take_materials(&id, materials),
            Ask::Call(_) | Ask::Again(..) | Ask::Used(..) | Ask::Data(_) => {}
        }
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer {
            reply,
            durable,
            fanout: None,
            next: None,
        }
    }
}
