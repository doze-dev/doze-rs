//! The state a Secrets Manager service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_secrets_manager::Operation;
use aws_seal::Sealer;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::model::Secret;

const FILE: &str = "secrets.redb";
const SECRETS: &str = "secrets";
const META: &str = "meta";
const SEAL_KEY: &str = "seal";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of Secrets Manager: its secrets, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) secrets: Records<Secret>,
    meta: Records<Vec<u8>>,
    pub(crate) sealer: Sealer,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first secret marked for deletion is due to go, so a call before then need not look.
    next_purge_ms: Option<i64>,
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
}

/// The operations this service runs. The others are refused by name before their request is read: replication to other
/// regions is not built (one account, one region each), and rotation drives a Lambda function, which waits for Lambda.
pub(crate) fn built(op: Operation) -> bool {
    !matches!(
        op,
        Operation::RotateSecret
            | Operation::CancelRotateSecret
            | Operation::ReplicateSecretToRegions
            | Operation::RemoveRegionsFromReplication
            | Operation::StopReplicationToReplica
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
    /// Secrets Manager with no disk and a fixed sealing key (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>) -> Option<Self> {
        let sealer = Sealer::new(&[7; aws_seal::KEY])?;
        let secrets = Records::new(SECRETS, "secret", VERSION);
        Some(Self::with(
            clock,
            secrets,
            Records::new(META, "meta", VERSION),
            sealer,
            None,
        ))
    }

    pub(crate) fn open_in(dir: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[SECRETS, META])?;
        let read = db.read()?;
        let secrets = Records::load(SECRETS, "secret", VERSION, &read)?;
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
        Ok(Self::with(clock, secrets, meta, sealer, Some(db)))
    }

    fn with(
        clock: Arc<dyn Clock>,
        secrets: Records<Secret>,
        meta: Records<Vec<u8>>,
        sealer: Sealer,
        db: Option<Db>,
    ) -> Self {
        let next_purge_ms = secrets.iter().filter_map(|(_, s)| s.purge_ms).min();
        Self {
            secrets,
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

    /// Notes that a secret is due to be removed at `at_ms`.
    pub(crate) fn purge_at(&mut self, at_ms: i64) {
        self.next_purge_ms = Some(self.next_purge_ms.map_or(at_ms, |n| n.min(at_ms)));
    }

    /// Removes the secrets whose recovery window has run out.
    fn sweep(&mut self) {
        let now = self.now_ms();
        if self.next_purge_ms.is_none_or(|at| at > now) {
            return;
        }
        let gone: Vec<String> = self
            .secrets
            .iter()
            .filter(|(_, s)| s.purge_ms.is_some_and(|at| at <= now))
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in gone {
            self.secrets.remove(&name);
        }
        self.next_purge_ms = self.secrets.iter().filter_map(|(_, s)| s.purge_ms).min();
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![self.secrets.stage()?, self.meta.stage()?];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over the model.
    pub(crate) fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            ));
        };
        self.sweep();
        match op {
            Operation::CreateSecret => self.create_secret(call),
            Operation::GetSecretValue => self.get_secret_value(call),
            Operation::BatchGetSecretValue => self.batch_get_secret_value(call),
            Operation::PutSecretValue => self.put_secret_value(call),
            Operation::UpdateSecret => self.update_secret(call),
            Operation::DescribeSecret => self.describe_secret(call),
            Operation::ListSecrets => self.list_secrets(call),
            Operation::ListSecretVersionIds => self.list_secret_version_ids(call),
            Operation::DeleteSecret => self.delete_secret(call),
            Operation::RestoreSecret => self.restore_secret(call),
            Operation::UpdateSecretVersionStage => self.update_secret_version_stage(call),
            Operation::TagResource => self.tag_resource(call),
            Operation::UntagResource => self.untag_resource(call),
            Operation::GetRandomPassword => self.get_random_password(call),
            Operation::PutResourcePolicy => self.put_resource_policy(call),
            Operation::GetResourcePolicy => self.get_resource_policy(call),
            Operation::DeleteResourcePolicy => self.delete_resource_policy(call),
            Operation::ValidateResourcePolicy => self.validate_resource_policy(call),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

pub(crate) fn not_found() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "Secrets Manager can't find the specified secret.",
    )
}

pub(crate) fn invalid_request(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidRequestException", message)
}

pub(crate) fn invalid_parameter(message: impl Into<String>) -> ApiError {
    ApiError::modeled("InvalidParameterException", message)
}

/// The words AWS gave for an operation on a secret that is marked for deletion (recorded 2026-11-09).
pub(crate) fn marked_for_deletion() -> ApiError {
    invalid_request(
        "You can't perform this operation on the secret because it was marked for deletion.",
    )
}
