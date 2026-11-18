//! Replicas in other regions: `ReplicateSecretToRegions`, `RemoveRegionsFromReplication`, `StopReplicationToReplica`, and
//! `AddReplicaRegions` on `CreateSecret`. A replica is a secret of the same name in another region (its ARN is the primary's with
//! the region changed), kept by that region's actor, sealed under a KMS key of its region (the one the replica names, or
//! `aws/secretsmanager`), and read-only there: every change is made on the primary and reaches the replica.
//!
//! What is replicated is done by the service outside the actors (`Secrets::ask`), from the [`Push`]es the primary's actor answers
//! with after a change: the replica's region takes the secret (its values open, to be sealed again under its own key), and the
//! primary is told how it went (`InSync`, or `Failed` with AWS's words: a secret of that name already in the region, unless the
//! call forced the overwrite). No actor waits on another. The values travel between the two in memory only.
//!
//! Kept in a table of its own (`replication`), so the secret record keeps the shape it had on disk.

use aws_model_secrets_manager::keys::{
    RemoveRegionsFromReplicationRequest as Remove, RemoveRegionsFromReplicationResponse as Removed,
    ReplicaRegionType, ReplicateSecretToRegionsRequest as Replicate,
    ReplicateSecretToRegionsResponse as Replicated, ReplicationStatusType as Status,
    StopReplicationToReplicaRequest as Stop, StopReplicationToReplicaResponse as Stopped,
};
use aws_schema::build::Object;
use aws_schema::view::kind::{List, Obj};
use aws_schema::view::{Key, View};
use aws_wire::ApiError;
use aws_wire::handler::{reply, request};
use aws_wire::service::{Call, Caller, Reply};
use serde::{Deserialize, Serialize};

use crate::core::{Core, invalid_parameter};
use crate::model::Secret;

/// A secret's place in replication.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Replication {
    /// For a replica: the region of its primary.
    pub(crate) primary: Option<String>,
    /// For a primary: its replicas.
    pub(crate) replicas: Vec<Replica>,
}

/// One replica, as its primary keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Replica {
    pub(crate) region: String,
    pub(crate) kms_key_id: Option<String>,
    /// `InProgress`, `InSync` or `Failed`.
    pub(crate) status: String,
    pub(crate) message: Option<String>,
}

/// A secret as it goes to a replica: the primary's record, and each version's value, open.
#[derive(Clone, Debug)]
pub(crate) struct Mirror {
    pub(crate) secret: Secret,
    pub(crate) values: Vec<(String, Vec<u8>)>,
    pub(crate) kms_key_id: Option<String>,
    pub(crate) primary: String,
    pub(crate) force: bool,
    pub(crate) caller: Caller,
}

/// Work a call leaves for another region.
#[derive(Debug)]
pub(crate) enum Push {
    /// The replica in `region` takes the secret now.
    Mirror(String, Box<Mirror>),
    /// The replica `name` in `region` is removed.
    Drop(String, String),
    /// The primary of `name` in `region` forgets its replica in `from`.
    Detach(String, String, String),
}

/// The words AWS gives for a change made on a replica (recorded: test_secretsmanager.py::test_replica_follows_its_primary).
pub(crate) fn read_only() -> ApiError {
    invalid_parameter(
        "Operation not permitted on a replica secret. Call must be made in primary secret's region.",
    )
}

/// The ARN `arn` has in `region`.
pub(crate) fn arn_in(arn: &str, region: &str) -> String {
    let mut parts: Vec<&str> = arn.splitn(5, ':').collect();
    if let Some(r) = parts.get_mut(3) {
        *r = region;
    }
    parts.join(":")
}

/// The regions a request adds, with the key each is sealed under.
pub(crate) fn regions_of<'a>(
    list: impl Iterator<Item = View<'a, ReplicaRegionType>>,
) -> Vec<(String, Option<String>)> {
    list.map(|r| {
        (
            r.get(ReplicaRegionType::REGION)
                .unwrap_or_default()
                .to_owned(),
            r.get(ReplicaRegionType::KMS_KEY_ID).map(str::to_owned),
        )
    })
    .collect()
}

/// A replica's status. As recorded (test_secretsmanager.py::test_replica_follows_its_primary): the key is named even when it is
/// the default one, and a replica in sync says so.
pub(crate) fn status_into(r: &Replica, o: &mut Object<'_, Status>) {
    o.put(Status::REGION, &r.region);
    o.put(
        Status::KMS_KEY_ID,
        r.kms_key_id.as_deref().unwrap_or(crate::kms::DEFAULT_KEY),
    );
    o.put(Status::STATUS, &r.status);
    let message = match (&r.message, r.status.as_str()) {
        (Some(m), _) => Some(m.as_str()),
        (None, "InSync") => Some("Replication succeeded"),
        (None, _) => None,
    };
    if let Some(m) = message {
        o.put(Status::STATUS_MESSAGE, m);
    }
}

impl Core {
    /// The region of the primary of `name`, when it is replicated (`own` when it is the primary).
    pub(crate) fn primary_region(&self, name: &str, own: &str) -> Option<String> {
        let rep = self.replication.get(name)?;
        match &rep.primary {
            Some(p) => Some(p.clone()),
            None if !rep.replicas.is_empty() => Some(own.to_owned()),
            None => None,
        }
    }

    /// Whether `name` is a replica here.
    pub(crate) fn is_replica(&self, name: &str) -> bool {
        self.replication
            .get(name)
            .is_some_and(|r| r.primary.is_some())
    }

    /// Adds `regions` as replicas of `name`, refusing the region it is in. A region it is already replicated to is not refused
    /// (recorded: test_secretsmanager.py::test_replica_follows_its_primary): the replica there is made again, `InProgress`.
    pub(crate) fn add_replicas(
        &mut self,
        call: &Call,
        name: &str,
        regions: Vec<(String, Option<String>)>,
    ) -> Result<(), ApiError> {
        let mut rep = self.replication.get(name).cloned().unwrap_or_default();
        for (region, key) in regions {
            if region == call.caller.region {
                // (memory)
                return Err(invalid_parameter(format!(
                    "The secret is already in {region}: a secret is not replicated to its own Region."
                )));
            }
            rep.replicas.retain(|r| r.region != region);
            rep.replicas.push(Replica {
                region,
                kms_key_id: key,
                status: "InProgress".to_owned(),
                message: None,
            });
        }
        self.replication.insert(name.to_owned(), rep);
        Ok(())
    }

    /// The replicas `CreateSecret` asks for (`AddReplicaRegions`), added to the new secret `name`.
    pub(crate) fn created_replicas<'a>(
        &mut self,
        call: &Call,
        name: &str,
        regions: Option<
            aws_schema::view::Items<'a, aws_schema::view::kind::Obj<ReplicaRegionType>>,
        >,
        force: Option<bool>,
    ) -> Result<Vec<Replica>, ApiError> {
        let regions = regions_of(regions.into_iter().flatten());
        if regions.is_empty() {
            return Ok(Vec::new());
        }
        self.add_replicas(call, name, regions)?;
        self.force_overwrite = force.unwrap_or(false);
        Ok(self
            .replication
            .get(name)
            .map(|r| r.replicas.clone())
            .unwrap_or_default())
    }

    pub(crate) fn replicate_secret_to_regions(&mut self, call: &Call) -> Reply {
        let v = request::<Replicate>(call)?;
        let name = self.locate_live(v.get(Replicate::SECRET_ID).unwrap_or_default())?;
        let regions = regions_of(v.get(Replicate::ADD_REPLICA_REGIONS).into_iter().flatten());
        self.add_replicas(call, &name, regions)?;
        self.force_overwrite = v
            .get(Replicate::FORCE_OVERWRITE_REPLICA_SECRET)
            .unwrap_or(false);
        let arn = self
            .secrets
            .get(&name)
            .map(|s| s.arn.clone())
            .unwrap_or_default();
        let rep = self.replication.get(&name).cloned().unwrap_or_default();
        reply::<Replicated>(|o| {
            o.put(Replicated::ARN, &arn);
            put_statuses(o, Replicated::REPLICATION_STATUS, &rep.replicas);
        })
    }

    pub(crate) fn remove_regions_from_replication(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let name = self.locate(v.get(Remove::SECRET_ID).unwrap_or_default())?;
        let gone: Vec<String> = v
            .get(Remove::REMOVE_REPLICA_REGIONS)
            .map(|r| r.map(str::to_owned).collect())
            .unwrap_or_default();
        let mut rep = self.replication.get(&name).cloned().unwrap_or_default();
        if let Some(region) = gone
            .iter()
            .find(|g| !rep.replicas.iter().any(|r| &r.region == *g))
        {
            // (memory)
            return Err(invalid_parameter(format!(
                "The secret has no replica in {region}."
            )));
        }
        rep.replicas.retain(|r| !gone.contains(&r.region));
        for region in gone {
            self.pushes.push(Push::Drop(region, name.clone()));
        }
        let arn = self
            .secrets
            .get(&name)
            .map(|s| s.arn.clone())
            .unwrap_or_default();
        if rep == Replication::default() {
            self.replication.remove(&name);
        } else {
            self.replication.insert(name.clone(), rep.clone());
        }
        // The list is there even when no replica is left (recorded).
        reply::<Removed>(|o| {
            o.put(Removed::ARN, &arn);
            o.put_list(Removed::REPLICATION_STATUS, |l| {
                for r in &rep.replicas {
                    l.push(|s| status_into(r, s));
                }
            });
        })
    }

    pub(crate) fn stop_replication_to_replica(&mut self, call: &Call) -> Reply {
        let v = request::<Stop>(call)?;
        let name = self.locate(v.get(Stop::SECRET_ID).unwrap_or_default())?;
        let Some(primary) = self.replication.get(&name).and_then(|r| r.primary.clone()) else {
            // Recorded (test_secretsmanager.py::test_replica_follows_its_primary).
            return Err(invalid_parameter(
                "Operation not permitted on a primary/standalone secret.",
            ));
        };
        self.replication.remove(&name);
        self.pushes.push(Push::Detach(
            primary,
            name.clone(),
            call.caller.region.clone(),
        ));
        let arn = self
            .secrets
            .get(&name)
            .map(|s| s.arn.clone())
            .unwrap_or_default();
        reply::<Stopped>(|o| {
            o.put(Stopped::ARN, &arn);
        })
    }

    /// After a change to `name`: the secret as it now is goes to each of its replicas.
    pub(crate) async fn mirror(&mut self, caller: &Caller, name: &str) -> Result<(), ApiError> {
        let Some(rep) = self
            .replication
            .get(name)
            .filter(|r| r.primary.is_none())
            .cloned()
        else {
            return Ok(());
        };
        let Some(secret) = self.secrets.get(name).cloned() else {
            return Ok(());
        };
        let mut values = Vec::new();
        for (id, version) in &secret.versions {
            if crate::rotate::has_value(version) {
                values.push((
                    id.clone(),
                    self.kms_open(caller, &secret, id, &version.sealed).await?,
                ));
            }
        }
        let force = std::mem::take(&mut self.force_overwrite);
        for r in rep.replicas {
            let mirror = Mirror {
                secret: secret.clone(),
                values: values.clone(),
                kms_key_id: r.kms_key_id.clone(),
                primary: caller.region.clone(),
                force,
                caller: Caller {
                    region: r.region.clone(),
                    ..caller.clone()
                },
            };
            self.pushes.push(Push::Mirror(r.region, Box::new(mirror)));
        }
        Ok(())
    }

    /// Takes a secret from its primary. Refused when a secret of the name that is not this primary's replica is here and the
    /// call did not force the overwrite.
    pub(crate) async fn take_mirror(&mut self, m: Mirror) -> Result<(), String> {
        let name = m.secret.name.clone();
        let ours = self
            .replication
            .get(&name)
            .and_then(|r| r.primary.as_deref())
            == Some(m.primary.as_str());
        if self.secrets.get(&name).is_some() && !ours && !m.force {
            // The words the model's documentation gives.
            return Err("Secret with this name already exists in this region".to_owned());
        }
        let mut secret = m.secret.clone();
        secret.arn = arn_in(&m.secret.arn, &m.caller.region);
        // A replica names its key even when it is the default one, and was made when it was made, not when its primary was
        // (recorded: test_secretsmanager.py::test_replica_follows_its_primary).
        secret.kms_key_id = Some(
            m.kms_key_id
                .clone()
                .unwrap_or_else(|| crate::kms::DEFAULT_KEY.to_owned()),
        );
        secret.created_ms = match self.secrets.get(&name).filter(|_| ours) {
            Some(existing) => existing.created_ms,
            None => self.now_ms(),
        };
        secret.policy = None;
        let kept = self
            .secrets
            .get(&name)
            .filter(|_| ours)
            .map(|s| s.accessed_day_ms);
        secret.accessed_day_ms = kept.flatten();
        for (id, plain) in &m.values {
            let sealed = self
                .kms_seal(&m.caller, &secret, id, plain)
                .await
                .map_err(|e| e.message.clone())?;
            if let Some(v) = secret.versions.get_mut(id) {
                v.sealed = sealed;
            }
        }
        self.secrets.insert(name.clone(), secret);
        self.replication.insert(
            name,
            Replication {
                primary: Some(m.primary),
                replicas: Vec::new(),
            },
        );
        Ok(())
    }

    /// Removes the replica `name`, if it is one.
    pub(crate) fn drop_replica(&mut self, name: &str) {
        if self.is_replica(name) {
            self.secrets.remove(name);
            self.replication.remove(name);
            self.rotations.remove(name);
        }
    }

    /// Notes that no replica of `name` could take the change (its values could not be opened here).
    pub(crate) fn mark_failed(&mut self, name: &str, message: &str) {
        if let Some(mut rep) = self.replication.get(name).cloned() {
            for r in &mut rep.replicas {
                r.status = "Failed".to_owned();
                r.message = Some(message.to_owned());
            }
            self.replication.insert(name.to_owned(), rep);
        }
        self.pushes.clear();
    }

    /// Notes how replicating `name` to `region` went, or that the replica there left (`detached`).
    pub(crate) fn replica_status(
        &mut self,
        name: &str,
        region: &str,
        outcome: Result<(), String>,
        detached: bool,
    ) {
        let Some(mut rep) = self.replication.get(name).cloned() else {
            return;
        };
        if detached {
            rep.replicas.retain(|r| r.region != region);
        } else if let Some(r) = rep.replicas.iter_mut().find(|r| r.region == region) {
            match outcome {
                Ok(()) => (r.status, r.message) = ("InSync".to_owned(), None),
                Err(m) => (r.status, r.message) = ("Failed".to_owned(), Some(m)),
            }
        }
        if rep == Replication::default() {
            self.replication.remove(name);
        } else if self.replication.get(name) != Some(&rep) {
            self.replication.insert(name.to_owned(), rep);
        }
    }
}

/// The replicas' statuses, under `key`, when there are any.
pub(crate) fn put_statuses<S>(
    o: &mut Object<'_, S>,
    key: Key<S, List<Obj<Status>>>,
    replicas: &[Replica],
) {
    if replicas.is_empty() {
        return;
    }
    o.put_list(key, |l| {
        for r in replicas {
            l.push(|s| status_into(r, s));
        }
    });
}
