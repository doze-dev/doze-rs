//! The state a Parameter Store service owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_ssm::Operation;
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};

use crate::doc_review::Reviews;
use crate::document::Document;
use crate::model::Parameter;
use crate::resource_policy::Held;
use crate::settings::Setting;
use aws_peers::{Mode, Peers};

const FILE: &str = "ssm.redb";
const PARAMS: &str = "params";
const DOCUMENTS: &str = "documents";
const SETTINGS: &str = "settings";
const POLICIES: &str = "resource_policies";
const REVIEWS: &str = "reviews";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
pub(crate) const VERSION: u8 = 1;

/// All of Parameter Store: its parameters, and the clock it reads. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) params: Records<Parameter>,
    /// SSM documents, by name.
    pub(crate) documents: Records<Document>,
    /// The service settings changed from their defaults, by id.
    pub(crate) settings: Records<Setting>,
    /// Resource policies, by the resource they are on (`parameter:<name>`, `document:<name>`).
    pub(crate) res_policies: Records<Held>,
    /// The reviews of change-template versions, by `<document>/<version>`.
    pub(crate) reviews: Records<Reviews>,
    /// The other services (KMS seals a `SecureString`; Secrets Manager answers a reference), when there are any.
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    /// When the first `Expiration` policy runs out, so a call before then need not look.
    next_expiry_ms: Option<i64>,
}

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Ask {
    /// A call from the wire (or the peer bus).
    Call(Box<Call>),
    /// From `doze-aws data`: what the parameters hold, or remove some of it.
    Data(Box<crate::storage::DataCall>),
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
            | Operation::CreateDocument
            | Operation::GetDocument
            | Operation::DescribeDocument
            | Operation::UpdateDocument
            | Operation::DeleteDocument
            | Operation::ListDocuments
            | Operation::ListDocumentVersions
            | Operation::UpdateDocumentDefaultVersion
            | Operation::DescribeDocumentPermission
            | Operation::ModifyDocumentPermission
            | Operation::UpdateDocumentMetadata
            | Operation::GetServiceSetting
            | Operation::UpdateServiceSetting
            | Operation::ResetServiceSetting
            | Operation::PutResourcePolicy
            | Operation::GetResourcePolicies
            | Operation::DeleteResourcePolicy
            | Operation::ListDocumentMetadataHistory
    )
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers_of(ctx))
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        let call = match ask {
            Ask::Call(call) => *call,
            Ask::Data(asked) => {
                let crate::storage::DataCall { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask)).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                return Answer {
                    reply: aws_wire::handler::empty(),
                    durable: None,
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
            Tables {
                params: Records::new(PARAMS, "parameter", VERSION),
                documents: Records::new(DOCUMENTS, "document", VERSION),
                settings: Records::new(SETTINGS, "setting", VERSION),
                res_policies: Records::new(POLICIES, "resource policy", VERSION),
                reviews: Records::new(REVIEWS, "document review", VERSION),
            },
            peers,
            None,
        )
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[PARAMS, DOCUMENTS, SETTINGS, POLICIES, REVIEWS])?;
        let read = db.read()?;
        let tables = Tables {
            params: Records::load(PARAMS, "parameter", VERSION, &read)?,
            documents: Records::load(DOCUMENTS, "document", VERSION, &read)?,
            settings: Records::load(SETTINGS, "setting", VERSION, &read)?,
            res_policies: Records::load(POLICIES, "resource policy", VERSION, &read)?,
            reviews: Records::load(REVIEWS, "document review", VERSION, &read)?,
        };
        drop(read);
        Ok(Self::with(clock, tables, peers, Some(db)))
    }

    fn with(clock: Arc<dyn Clock>, t: Tables, peers: Option<Arc<Peers>>, db: Option<Db>) -> Self {
        let next_expiry_ms = t.params.iter().filter_map(|(_, p)| p.expires_ms).min();
        Self {
            params: t.params,
            documents: t.documents,
            settings: t.settings,
            res_policies: t.res_policies,
            reviews: t.reviews,
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

    /// After a reset: nothing is due to expire.
    pub(crate) fn forget_expiry(&mut self) {
        self.next_expiry_ms = None;
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
            self.forget_policies(&crate::resource_policy::parameter_key(&name));
        }
        self.next_expiry_ms = self.params.iter().filter_map(|(_, p)| p.expires_ms).min();
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.params.stage()?,
            self.documents.stage()?,
            self.settings.stage()?,
            self.res_policies.stage()?,
            self.reviews.stage()?,
        ];
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
            _ => self.dispatch_more(op, call),
        }
    }

    /// The documents and the service settings.
    fn dispatch_more(&mut self, op: Operation, call: &Call) -> Reply {
        match op {
            Operation::CreateDocument => self.create_document(call),
            Operation::GetDocument => self.get_document(call),
            Operation::DescribeDocument => self.describe_document(call),
            Operation::UpdateDocument => self.update_document(call),
            Operation::DeleteDocument => self.delete_document(call),
            Operation::ListDocuments => self.list_documents(call),
            Operation::ListDocumentVersions => self.list_document_versions(call),
            Operation::UpdateDocumentDefaultVersion => self.update_document_default_version(call),
            Operation::DescribeDocumentPermission => self.describe_document_permission(call),
            Operation::ModifyDocumentPermission => self.modify_document_permission(call),
            Operation::UpdateDocumentMetadata => self.update_document_metadata(call),
            Operation::GetServiceSetting => self.get_service_setting(call),
            Operation::UpdateServiceSetting => self.update_service_setting(call),
            Operation::ResetServiceSetting => self.reset_service_setting(call),
            Operation::PutResourcePolicy => self.put_resource_policy(call),
            Operation::GetResourcePolicies => self.get_resource_policies(call),
            Operation::DeleteResourcePolicy => self.delete_resource_policy(call),
            Operation::ListDocumentMetadataHistory => self.list_document_metadata_history(call),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

/// The tables the service keeps.
struct Tables {
    params: Records<Parameter>,
    documents: Records<Document>,
    settings: Records<Setting>,
    res_policies: Records<Held>,
    reviews: Records<Reviews>,
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
