//! The state a KMS service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_kms::Operation;
use aws_seal::Sealer;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::model::{Alias, Key};

const FILE: &str = "kms.redb";
const KEYS: &str = "keys";
const ALIASES: &str = "aliases";
const META: &str = "meta";
const SEAL_KEY: &str = "seal";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of KMS: its keys and aliases, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) keys: Records<Key>,
    pub(crate) aliases: Records<Alias>,
    meta: Records<Vec<u8>>,
    pub(crate) sealer: Sealer,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first key scheduled for deletion is due to go, so a call before then need not look.
    next_purge_ms: Option<i64>,
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
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
    )
}

impl doze_runtime::Service for Core {
    type Request = Call;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock))
    }

    async fn handle(&mut self, call: Call) -> Answer {
        let mut reply = self.dispatch(&call);
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer { reply, durable }
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
            ),
            sealer,
            None,
        ))
    }

    pub(crate) fn open_in(dir: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[KEYS, ALIASES, META])?;
        let read = db.read()?;
        let keys = Records::load(KEYS, "key", VERSION, &read)?;
        let aliases = Records::load(ALIASES, "alias", VERSION, &read)?;
        let mut meta: Records<Vec<u8>> = Records::load(META, "meta", VERSION, &read)?;
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
        Ok(Self::with(clock, (keys, aliases, meta), sealer, Some(db)))
    }

    fn with(
        clock: Arc<dyn Clock>,
        (keys, aliases, meta): (Records<Key>, Records<Alias>, Records<Vec<u8>>),
        sealer: Sealer,
        db: Option<Db>,
    ) -> Self {
        let next_purge_ms = keys.iter().filter_map(|(_, k)| k.purge_ms).min();
        Self {
            keys,
            aliases,
            meta,
            sealer,
            clock,
            db,
            next_purge_ms,
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
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
        let orphans: Vec<String> = self
            .aliases
            .iter()
            .filter(|(_, a)| gone.contains(&a.key_id))
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in orphans {
            self.aliases.remove(&name);
        }
        self.next_purge_ms = self.keys.iter().filter_map(|(_, k)| k.purge_ms).min();
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.keys.stage()?,
            self.aliases.stage()?,
            self.meta.stage()?,
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
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}
