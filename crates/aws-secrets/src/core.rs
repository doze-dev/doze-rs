//! The state a Secrets Manager service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_secrets_manager::Operation;
use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::model::{Rotation, Secret};
use crate::replication::{Mirror, Push, Replication, read_only};
use crate::rotate::Run;

pub(crate) const FILE: &str = "secrets.redb";
const SECRETS: &str = "secrets";
const ROTATIONS: &str = "rotations";
const REPLICATION: &str = "replication";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
pub(crate) const VERSION: u8 = 1;

/// All of Secrets Manager: its secrets, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) secrets: Records<Secret>,
    pub(crate) rotations: Records<Rotation>,
    /// Each replicated secret's place in replication: its replicas, or its primary.
    pub(crate) replication: Records<Replication>,
    /// What the call leaves for other regions, done once it is saved.
    pub(crate) pushes: Vec<Push>,
    /// Whether the call asked to overwrite a secret of the same name in a replica's region.
    pub(crate) force_overwrite: bool,
    /// The rotations the call (or the timer's tick) just started, run after the answer is saved.
    pub(crate) runs: Vec<Run>,
    /// The other services (KMS keeps every secret's value), when there are any.
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first secret marked for deletion is due to go, so a call before then need not look.
    next_purge_ms: Option<i64>,
}

/// What the actor is asked: a call from a client, or the rotation timer's tick (start every rotation that is due).
#[derive(Debug)]
pub(crate) enum Request {
    Call(Box<Call>),
    Tick,
    /// From `doze-aws data`: what the secrets hold, or remove some of it.
    Data(Box<crate::storage::DataCall>),
    /// From a primary's region: take this secret as its replica.
    Mirror(Box<Mirror>),
    /// From a primary's region: remove this replica.
    Drop(String),
    /// To a primary's region: how replicating a secret to a region went, or that the replica there left.
    Status {
        name: String,
        region: String,
        outcome: Result<(), String>,
        detached: bool,
    },
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
    /// The rotations to run once the answer is saved.
    pub(crate) runs: Vec<Run>,
    /// When the next scheduled rotation of the region is due, for the region's timer.
    pub(crate) next_due: Option<i64>,
    /// What other regions are to do once the answer is saved.
    pub(crate) pushes: Vec<Push>,
}

/// The operations this service runs: every one of the model.
pub(crate) const fn built(_op: Operation) -> bool {
    true
}

/// The operations that change a secret: refused on a replica, and followed by the replicas of a primary.
fn changes(op: Operation) -> bool {
    use Operation as O;
    matches!(
        op,
        O::PutSecretValue
            | O::UpdateSecret
            | O::UpdateSecretVersionStage
            | O::TagResource
            | O::UntagResource
            | O::DeleteSecret
            | O::RestoreSecret
            | O::RotateSecret
            | O::CancelRotateSecret
            | O::PutResourcePolicy
            | O::DeleteResourcePolicy
            | O::ReplicateSecretToRegions
            | O::RemoveRegionsFromReplication
    )
}

impl doze_runtime::Service for Core {
    type Request = Request;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, request: Request) -> Answer {
        let call = match request {
            Request::Call(call) => *call,
            Request::Tick => return self.tick(),
            Request::Mirror(_) | Request::Drop(_) | Request::Status { .. } => {
                return self.region_work(request).await;
            }
            Request::Data(asked) => {
                let crate::storage::DataCall { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask)).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                return Answer {
                    reply: aws_wire::handler::empty(),
                    durable: None,
                    runs: Vec::new(),
                    next_due: self.next_due(),
                    pushes: Vec::new(),
                };
            }
        };
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer {
                reply: Err(refusal),
                durable: None,
                runs: Vec::new(),
                next_due: self.next_due(),
                pushes: Vec::new(),
            };
        }
        let mut reply = self.guarded(&call).await;
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        let runs = std::mem::take(&mut self.runs);
        let runs = if reply.is_ok() { runs } else { Vec::new() };
        let pushes = std::mem::take(&mut self.pushes);
        let pushes = if reply.is_ok() { pushes } else { Vec::new() };
        Answer {
            reply,
            durable,
            runs,
            next_due: self.next_due(),
            pushes,
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// Secrets Manager with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(
            clock,
            (
                Records::new(SECRETS, "secret", VERSION),
                Records::new(ROTATIONS, "rotation", VERSION),
                Records::new(REPLICATION, "replication", VERSION),
            ),
            peers,
            None,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[SECRETS, ROTATIONS, REPLICATION])?;
        let read = db.read()?;
        let secrets = Records::load(SECRETS, "secret", VERSION, &read)?;
        let rotations = Records::load(ROTATIONS, "rotation", VERSION, &read)?;
        let replication = Records::load(REPLICATION, "replication", VERSION, &read)?;
        drop(read);
        Ok(Self::with(
            clock,
            (secrets, rotations, replication),
            peers,
            Some(db),
        ))
    }

    fn with(
        clock: Arc<dyn Clock>,
        (secrets, rotations, replication): (
            Records<Secret>,
            Records<Rotation>,
            Records<Replication>,
        ),
        peers: Option<Arc<Peers>>,
        db: Option<Db>,
    ) -> Self {
        let next_purge_ms = secrets.iter().filter_map(|(_, s)| s.purge_ms).min();
        Self {
            secrets,
            rotations,
            replication,
            pushes: Vec::new(),
            force_overwrite: false,
            runs: Vec::new(),
            peers,
            clock,
            db,
            next_purge_ms,
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// After a reset: no secret is due to be removed.
    pub(crate) fn forget_due(&mut self) {
        self.next_purge_ms = None;
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
            self.replication.remove(&name);
        }
        self.next_purge_ms = self.secrets.iter().filter_map(|(_, s)| s.purge_ms).min();
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.secrets.stage()?,
            self.rotations.stage()?,
            self.replication.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over the model.
    pub(crate) async fn dispatch(&mut self, call: &Call) -> Reply {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            ));
        };
        self.sweep();
        match op {
            Operation::CreateSecret => self.create_secret(call).await,
            Operation::GetSecretValue => self.get_secret_value(call).await,
            Operation::BatchGetSecretValue => self.batch_get_secret_value(call).await,
            Operation::PutSecretValue => self.put_secret_value(call).await,
            Operation::UpdateSecret => self.update_secret(call).await,
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
            Operation::RotateSecret => self.rotate_secret(call).await,
            Operation::CancelRotateSecret => self.cancel_rotate_secret(call),
            Operation::ReplicateSecretToRegions => self.replicate_secret_to_regions(call),
            Operation::RemoveRegionsFromReplication => self.remove_regions_from_replication(call),
            Operation::StopReplicationToReplica => self.stop_replication_to_replica(call),
        }
    }
}

impl Core {
    /// Runs a call: a change to a replica is refused, and a change to a primary goes to its replicas.
    async fn guarded(&mut self, call: &Call) -> Reply {
        let op = Operation::from_index(call.op);
        let target = target_of(call).and_then(|id| self.locate(&id).ok());
        if op.is_some_and(changes) && target.as_deref().is_some_and(|n| self.is_replica(n)) {
            return Err(read_only());
        }
        let reply = self.dispatch(call).await;
        let follows = op.is_some_and(|o| changes(o) || o == Operation::CreateSecret);
        if reply.is_ok()
            && follows
            && let Some(name) = target.or_else(|| target_of(call))
            && let Err(e) = self.mirror(&call.caller, &name).await
        {
            // WHY: the change is made; a replica that cannot follow it says so in its status.
            self.mark_failed(&name, &e.message);
        }
        reply
    }

    /// Work a primary or a replica in another region hands this one.
    async fn region_work(&mut self, request: Request) -> Answer {
        let reply = match request {
            Request::Mirror(m) => self
                .take_mirror(*m)
                .await
                .map_err(invalid_request)
                .and_then(|()| aws_wire::handler::empty()),
            Request::Drop(name) => {
                self.drop_replica(&name);
                aws_wire::handler::empty()
            }
            Request::Status {
                name,
                region,
                outcome,
                detached,
            } => {
                self.replica_status(&name, &region, outcome, detached);
                aws_wire::handler::empty()
            }
            _ => aws_wire::handler::empty(),
        };
        let durable = self.flush().ok().flatten();
        Answer {
            reply,
            durable,
            runs: Vec::new(),
            next_due: self.next_due(),
            pushes: Vec::new(),
        }
    }
}

/// The secret a request names (`SecretId`, or `Name` for `CreateSecret`).
fn target_of(call: &Call) -> Option<String> {
    let body: serde_json::Value = serde_json::from_str(&call.body).ok()?;
    body.get("SecretId")
        .or_else(|| body.get("Name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
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

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
