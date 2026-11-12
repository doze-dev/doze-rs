//! The state a Parameter Store service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_ssm::Operation;
use aws_schema::arena::Arena;
use aws_schema::build::Object;
use aws_schema::view::{Shape, View};
use aws_wire::ApiError;
use aws_wire::service::{Call, Output, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::model::Parameter;
use crate::seal::{self, Sealer};

const FILE: &str = "ssm.redb";
const PARAMS: &str = "params";
const META: &str = "meta";
const SEAL_KEY: &str = "seal";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// All of Parameter Store: its parameters, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) params: Records<Parameter>,
    meta: Records<Vec<u8>>,
    pub(crate) sealer: Sealer,
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
    /// Parameter Store with no disk and a fixed sealing key (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>) -> Option<Self> {
        let sealer = Sealer::new(&[7; seal::KEY])?;
        let params = Records::new(PARAMS, "parameter", VERSION);
        Some(Self::with(
            clock,
            params,
            Records::new(META, "meta", VERSION),
            sealer,
            None,
        ))
    }

    pub(crate) fn open_in(dir: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[PARAMS, META])?;
        let read = db.read()?;
        let params = Records::load(PARAMS, "parameter", VERSION, &read)?;
        let mut meta: Records<Vec<u8>> = Records::load(META, "meta", VERSION, &read)?;
        drop(read);
        if meta.get(SEAL_KEY).is_none() {
            let key = seal::new_key().ok_or_else(|| StoreError::Corrupt("no randomness".into()))?;
            meta.insert(SEAL_KEY.to_owned(), key);
        }
        let sealer = meta
            .get(SEAL_KEY)
            .and_then(|key| Sealer::new(key))
            .ok_or_else(|| StoreError::Corrupt("the sealing key is unreadable".into()))?;
        Ok(Self::with(clock, params, meta, sealer, Some(db)))
    }

    fn with(
        clock: Arc<dyn Clock>,
        params: Records<Parameter>,
        meta: Records<Vec<u8>>,
        sealer: Sealer,
        db: Option<Db>,
    ) -> Self {
        let next_expiry_ms = params.iter().filter_map(|(_, p)| p.expires_ms).min();
        Self {
            params,
            meta,
            sealer,
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
        let changes = vec![self.params.stage()?, self.meta.stage()?];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Every operation of the model is either run here or refused by name; the dispatch is exhaustive over those it runs.
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
            Operation::PutParameter => self.put_parameter(call),
            Operation::GetParameter => self.get_parameter(call),
            Operation::GetParameters => self.get_parameters(call),
            Operation::GetParametersByPath => self.get_parameters_by_path(call),
            Operation::GetParameterHistory => self.get_parameter_history(call),
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

/// The request as a typed view.
pub(crate) fn request<S: Shape>(call: &Call) -> Result<View<'_, S>, ApiError> {
    View::<S>::root(&call.arena, &call.body, call.root)
        .ok_or_else(|| ApiError::internal("the request was not decoded"))
}

/// An answer, written by `fill`.
pub(crate) fn reply<S>(fill: impl FnOnce(&mut Object<'_, S>)) -> Reply {
    let mut arena = Arena::new();
    let mut out = Object::<S>::root(&mut arena);
    fill(&mut out);
    let root = out.finish();
    Ok(Output { arena, root })
}

/// An answer with nothing in it.
pub(crate) fn empty() -> Reply {
    Ok(Output {
        arena: Arena::new(),
        root: aws_schema::arena::NIL,
    })
}

/// An operation's input that was left out. The model's `required` checks catch these first; this is for a member the model
/// leaves optional that the operation cannot do without.
pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}
