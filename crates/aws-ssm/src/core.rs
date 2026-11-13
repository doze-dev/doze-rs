//! The state a Parameter Store service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_ssm::Operation;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::model::Parameter;
use aws_peers::{Mode, Peers};

const FILE: &str = "ssm.redb";
const PARAMS: &str = "params";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of Parameter Store: its parameters, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) params: Records<Parameter>,
    /// The other services (KMS seals a `SecureString`; Secrets Manager answers a reference), when there are any.
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first `Expiration` policy runs out, so a call before then need not look.
    next_expiry_ms: Option<i64>,
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
}

/// The operations this service runs. Every other is refused by name before its request is read.
pub(crate) fn built(op: Operation) -> bool {
    matches!(
        op,
        Operation::PutParameter
            | Operation::GetParameter
            | Operation::GetParameters
            | Operation::GetParametersByPath
            | Operation::GetParameterHistory
            | Operation::DeleteParameter
            | Operation::DeleteParameters
            | Operation::DescribeParameters
            | Operation::LabelParameterVersion
            | Operation::UnlabelParameterVersion
            | Operation::AddTagsToResource
            | Operation::RemoveTagsFromResource
            | Operation::ListTagsForResource
    )
}

impl doze_runtime::Service for Core {
    type Request = Call;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, call: Call) -> Answer {
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
            && let Err(refusal) = peers
                .authorize(&call.caller, crate::authz::needs(&call))
                .await
        {
            return Answer {
                reply: Err(refusal),
                durable: None,
            };
        }
        let mut reply = self.dispatch(&call).await;
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
    /// Parameter Store with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(
            clock,
            Records::new(PARAMS, "parameter", VERSION),
            peers,
            None,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[PARAMS])?;
        let read = db.read()?;
        let params = Records::load(PARAMS, "parameter", VERSION, &read)?;
        drop(read);
        Ok(Self::with(clock, params, peers, Some(db)))
    }

    fn with(
        clock: Arc<dyn Clock>,
        params: Records<Parameter>,
        peers: Option<Arc<Peers>>,
        db: Option<Db>,
    ) -> Self {
        let next_expiry_ms = params.iter().filter_map(|(_, p)| p.expires_ms).min();
        Self {
            params,
            peers,
            clock,
            db,
            next_expiry_ms,
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Notes that a parameter now expires at `at_ms`.
    pub(crate) fn expires_at(&mut self, at_ms: i64) {
        self.next_expiry_ms = Some(self.next_expiry_ms.map_or(at_ms, |n| n.min(at_ms)));
    }

    /// Removes the parameters whose `Expiration` policy has run out.
    fn sweep(&mut self) {
        let now = self.now_ms();
        if self.next_expiry_ms.is_none_or(|at| at > now) {
            return;
        }
        let gone: Vec<String> = self
            .params
            .iter()
            .filter(|(_, p)| p.expired(now))
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in gone {
            self.params.remove(&name);
        }
        self.next_expiry_ms = self.params.iter().filter_map(|(_, p)| p.expires_ms).min();
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![self.params.stage()?];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over those it runs.
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
            Operation::PutParameter => self.put_parameter(call).await,
            Operation::GetParameter => self.get_parameter(call).await,
            Operation::GetParameters => self.get_parameters(call).await,
            Operation::GetParametersByPath => self.get_parameters_by_path(call).await,
            Operation::GetParameterHistory => self.get_parameter_history(call).await,
            Operation::DeleteParameter => self.delete_parameter(call),
            Operation::DeleteParameters => self.delete_parameters(call),
            Operation::DescribeParameters => self.describe_parameters(call),
            Operation::LabelParameterVersion => self.label_parameter_version(call),
            Operation::UnlabelParameterVersion => self.unlabel_parameter_version(call),
            Operation::AddTagsToResource => self.add_tags(call),
            Operation::RemoveTagsFromResource => self.remove_tags(call),
            Operation::ListTagsForResource => self.list_tags(call),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

/// A refusal that names the rule.
pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
