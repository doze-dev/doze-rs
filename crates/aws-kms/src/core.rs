//! The state a KMS service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_kms::Operation;
use aws_peers::{Mode, Peers};
use aws_seal::Sealer;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::agree::Used;
use crate::model::{Alias, Key};
use crate::multi::{Fanout, Open, Seed, Set};

const FILE: &str = "kms.redb";
const KEYS: &str = "keys";
const ALIASES: &str = "aliases";
const META: &str = "meta";
const SETS: &str = "multiregion";
const SEAL_KEY: &str = "seal";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of KMS: its keys and aliases, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) keys: Records<Key>,
    pub(crate) aliases: Records<Alias>,
    meta: Records<Vec<u8>>,
    /// The sets of the multi-Region keys this region holds, by key id.
    pub(crate) sets: Records<Set>,
    /// What the last call leaves for other regions.
    pub(crate) fanout: Option<Fanout>,
    /// Each key's last successful cryptographic use since this actor started (in memory only).
    pub(crate) used: std::collections::HashMap<String, Used>,
    /// When this actor started: when last use began to be tracked.
    pub(crate) started_ms: i64,
    pub(crate) sealer: Sealer,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first key scheduled for deletion is due to go, so a call before then need not look.
    next_purge_ms: Option<i64>,
    /// The bus, for asking whether a caller may do what it asks.
    pub(crate) peers: Option<Arc<Peers>>,
    /// What the call in hand was given from off the loop, and leaves to be done there.
    pub(crate) offload: crate::offload::Offload,
}

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Ask {
    /// A call from the wire (or the peer bus).
    Call(Box<Call>),
    /// A call asked again with what its job made off the loop.
    Again(Box<Call>, crate::offload::Made),
    /// A key's last use, noted once an answer made off the loop succeeded.
    Used(String, Used),
    /// From `doze-aws data`: what KMS holds, or remove it all.
    Data(Box<crate::storage::DataCall>),
    /// From the region of a multi-Region primary: make this replica.
    Replica(Box<Seed>),
    /// From another region of a multi-Region key: the set's new shape.
    Set(String, Set),
    /// From a multi-Region primary: its material after a rotation.
    Materials(String, Vec<Open>),
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    /// What other regions are to do before the call is answered.
    pub(crate) fanout: Option<Fanout>,
    /// What is left to do off the loop before the call is answered.
    pub(crate) next: Option<crate::offload::Next>,
}

/// The operations this service runs. The others are refused by name before their request is read.
pub(crate) fn built(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::CreateKey
            | O::DescribeKey
            | O::ListKeys
            | O::EnableKey
            | O::DisableKey
            | O::ScheduleKeyDeletion
            | O::CancelKeyDeletion
            | O::UpdateKeyDescription
            | O::Encrypt
            | O::Decrypt
            | O::ReEncrypt
            | O::GenerateDataKey
            | O::GenerateDataKeyWithoutPlaintext
            | O::GenerateRandom
            | O::Sign
            | O::Verify
            | O::GetPublicKey
            | O::GenerateMac
            | O::VerifyMac
            | O::CreateAlias
            | O::UpdateAlias
            | O::DeleteAlias
            | O::ListAliases
            | O::TagResource
            | O::UntagResource
            | O::ListResourceTags
            | O::GetKeyPolicy
            | O::PutKeyPolicy
            | O::ListKeyPolicies
            | O::GetKeyRotationStatus
            | O::EnableKeyRotation
            | O::DisableKeyRotation
            | O::RotateKeyOnDemand
            | O::ListKeyRotations
            | O::CreateGrant
            | O::ListGrants
            | O::ListRetirableGrants
            | O::RevokeGrant
            | O::RetireGrant
            | O::GenerateDataKeyPair
            | O::GenerateDataKeyPairWithoutPlaintext
            | O::GetParametersForImport
            | O::ImportKeyMaterial
            | O::DeleteImportedKeyMaterial
            | O::DeriveSharedSecret
            | O::ReplicateKey
            | O::UpdatePrimaryRegion
            | O::GetKeyLastUsage
    )
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(&ctx.dir, Arc::clone(&ctx.clock))?;
        core.peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        Ok(core)
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        let call = match ask {
            Ask::Call(call) => *call,
            Ask::Again(call, made) => {
                self.offload.made = Some(made);
                *call
            }
            Ask::Used(id, used) => {
                self.used.insert(id, used);
                return Answer::of(aws_wire::handler::empty());
            }
            Ask::Data(asked) => {
                let crate::storage::DataCall { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask)).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                return Answer::of(aws_wire::handler::empty());
            }
            other => return self.region_work(other),
        };
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer::of(Err(refusal));
        }
        let mut reply = self.dispatch(&call);
        let pending = self.pending(reply.is_ok());
        if reply.is_ok() && pending.is_none() {
            self.note_use(&call);
        }
        let fanout = self.fanout.take().filter(|_| reply.is_ok());
        let (durable, next) = match self.flush() {
            Ok(ack) => (ack, pending.map(|p| p.next(call))),
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                (None, None)
            }
        };
        Answer {
            reply,
            durable,
            fanout,
            next,
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// KMS with no disk and a fixed sealing key (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>) -> Option<Self> {
        let sealer = Sealer::new(&[7; aws_seal::KEY])?;
        Some(Self::with(
            clock,
            (
                Records::new(KEYS, "key", VERSION),
                Records::new(ALIASES, "alias", VERSION),
                Records::new(META, "meta", VERSION),
                Records::new(SETS, "set", VERSION),
            ),
            sealer,
            None,
        ))
    }

    pub(crate) fn open_in(dir: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[KEYS, ALIASES, META, SETS])?;
        let read = db.read()?;
        let keys = Records::load(KEYS, "key", VERSION, &read)?;
        let aliases = Records::load(ALIASES, "alias", VERSION, &read)?;
        let mut meta: Records<Vec<u8>> = Records::load(META, "meta", VERSION, &read)?;
        let sets = Records::load(SETS, "set", VERSION, &read)?;
        drop(read);
        if meta.get(SEAL_KEY).is_none() {
            let key =
                aws_seal::new_key().ok_or_else(|| StoreError::Corrupt("no randomness".into()))?;
            meta.insert(SEAL_KEY.to_owned(), key);
        }
        let sealer = meta
            .get(SEAL_KEY)
            .and_then(|key| Sealer::new(key))
            .ok_or_else(|| StoreError::Corrupt("the sealing key is unreadable".into()))?;
        Ok(Self::with(
            clock,
            (keys, aliases, meta, sets),
            sealer,
            Some(db),
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        (keys, aliases, meta, sets): Tables,
        sealer: Sealer,
        db: Option<Db>,
    ) -> Self {
        let next_purge_ms = next_event(&keys);
        let started_ms = clock.now_ms();
        Self {
            keys,
            aliases,
            meta,
            sets,
            fanout: None,
            used: std::collections::HashMap::new(),
            started_ms,
            sealer,
            clock,
            db,
            next_purge_ms,
            peers: None,
            offload: crate::offload::Offload::default(),
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// How many keys and aliases there are, and the bytes of their rows.
    pub(crate) fn count(&self) -> (u64, u64) {
        let (k, a, s) = (
            crate::storage::tally(&self.keys),
            crate::storage::tally(&self.aliases),
            crate::storage::tally(&self.sets),
        );
        // A multi-Region set is part of its key: its bytes count, not as an item of its own.
        (k.0 + a.0, k.1 + a.1 + s.1)
    }

    /// Removes every key, alias and the sealing key, and makes a new sealing key: KMS as a fresh data directory has it.
    pub(crate) fn wipe(&mut self) -> Result<(), StoreError> {
        crate::storage::clear(&mut self.keys);
        crate::storage::clear(&mut self.aliases);
        crate::storage::clear(&mut self.meta);
        crate::storage::clear(&mut self.sets);
        self.used.clear();
        drop(self.flush()?);
        let key = aws_seal::new_key().ok_or_else(|| StoreError::Corrupt("no randomness".into()))?;
        self.sealer = Sealer::new(&key)
            .ok_or_else(|| StoreError::Corrupt("the sealing key is unreadable".into()))?;
        self.meta.insert(SEAL_KEY.to_owned(), key);
        self.next_purge_ms = None;
        drop(self.flush()?);
        Ok(())
    }

    /// Notes that a key is due to be removed at `at_ms`.
    pub(crate) fn purge_at(&mut self, at_ms: i64) {
        self.next_purge_ms = Some(self.next_purge_ms.map_or(at_ms, |n| n.min(at_ms)));
    }

    /// Removes the keys whose deletion window has run out, and the aliases that pointed at them.
    fn sweep(&mut self) {
        let now = self.now_ms();
        if self.next_purge_ms.is_none_or(|at| at > now) {
            return;
        }
        let gone: Vec<String> = self
            .keys
            .iter()
            .filter(|(_, k)| k.purge_ms.is_some_and(|at| at <= now))
            .map(|(id, _)| id.to_owned())
            .collect();
        for id in &gone {
            self.keys.remove(id);
        }
        self.expire_imports(now);
        let orphans: Vec<String> = self
            .aliases
            .iter()
            .filter(|(_, a)| gone.contains(&a.key_id))
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in orphans {
            self.aliases.remove(&name);
        }
        self.next_purge_ms = next_event(&self.keys);
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.keys.stage()?,
            self.aliases.stage()?,
            self.meta.stage()?,
            self.sets.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is either run here or refused by name.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        use Operation as O;
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            ));
        };
        self.sweep();
        match op {
            O::CreateKey => self.create_key(call),
            O::DescribeKey => self.describe_key(call),
            O::ListKeys => self.list_keys(call),
            O::EnableKey => self.enable_key(call),
            O::DisableKey => self.disable_key(call),
            O::ScheduleKeyDeletion => self.schedule_key_deletion(call),
            O::CancelKeyDeletion => self.cancel_key_deletion(call),
            O::UpdateKeyDescription => self.update_key_description(call),
            O::Encrypt => self.encrypt(call),
            O::Decrypt => self.decrypt(call),
            O::ReEncrypt => self.re_encrypt(call),
            O::GenerateDataKey => self.generate_data_key(call),
            O::GenerateDataKeyWithoutPlaintext => self.generate_data_key_without_plaintext(call),
            O::GenerateRandom => self.generate_random(call),
            O::Sign => self.sign(call),
            O::Verify => self.verify(call),
            O::GetPublicKey => self.get_public_key(call),
            O::GenerateMac => self.generate_mac(call),
            O::VerifyMac => self.verify_mac(call),
            O::CreateAlias => self.create_alias(call),
            O::UpdateAlias => self.update_alias(call),
            O::DeleteAlias => self.delete_alias(call),
            O::ListAliases => self.list_aliases(call),
            O::TagResource => self.tag_resource(call),
            O::UntagResource => self.untag_resource(call),
            O::ListResourceTags => self.list_resource_tags(call),
            O::GetKeyPolicy => self.get_key_policy(call),
            O::PutKeyPolicy => self.put_key_policy(call),
            O::ListKeyPolicies => self.list_key_policies(call),
            O::GetKeyRotationStatus => self.get_key_rotation_status(call),
            O::EnableKeyRotation => self.enable_key_rotation(call),
            O::DisableKeyRotation => self.disable_key_rotation(call),
            O::RotateKeyOnDemand => self.rotate_key_on_demand(call),
            O::ListKeyRotations => self.list_key_rotations(call),
            O::CreateGrant => self.create_grant(call),
            O::ListGrants => self.list_grants(call),
            O::ListRetirableGrants => self.list_retirable_grants(call),
            O::RevokeGrant => self.revoke_grant(call),
            O::RetireGrant => self.retire_grant(call),
            O::GenerateDataKeyPair => self.generate_data_key_pair(call),
            O::GetParametersForImport => self.get_parameters_for_import(call),
            O::ImportKeyMaterial => self.import_key_material(call),
            O::DeleteImportedKeyMaterial => self.delete_imported_key_material(call),
            O::GenerateDataKeyPairWithoutPlaintext => {
                self.generate_data_key_pair_without_plaintext(call)
            }
            O::DeriveSharedSecret => self.derive_shared_secret(call),
            O::ReplicateKey => self.replicate_key(call),
            O::UpdatePrimaryRegion => self.update_primary_region(call),
            O::GetKeyLastUsage => self.get_key_last_usage(call),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

/// The tables KMS keeps: keys, aliases, its own sealing key, and the sets of multi-Region keys.
type Tables = (Records<Key>, Records<Alias>, Records<Vec<u8>>, Records<Set>);

/// When the next key is due to be removed or has imported material that expires.
fn next_event(keys: &Records<Key>) -> Option<i64> {
    keys.iter()
        .flat_map(|(_, k)| [k.purge_ms, k.valid_to_ms])
        .flatten()
        .min()
}

pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}
