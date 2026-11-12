//! The state an SNS region owns, and the dispatch from an operation to the code that does it.

use std::path::Path;
use std::sync::Arc;

use aws_model_sns::Operation;
use aws_model_sns::tables::TABLES;
use aws_peers::{Mode, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{Call, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::model::{Application, Endpoint, Sms, Subscription, Topic};
use crate::req::Req;

const FILE: &str = "sns.redb";
const TOPICS: &str = "topics";
const SUBSCRIPTIONS: &str = "subscriptions";
const APPLICATIONS: &str = "applications";
const ENDPOINTS: &str = "endpoints";
const SMS: &str = "sms";
/// The one record the SMS settings are kept in.
pub(crate) const SMS_KEY: &str = "sms";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// One region of SNS: its topics, subscriptions, applications and endpoints, and the clock it reads. Owned by one task, so no operation
/// needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) topics: Records<Topic>,
    pub(crate) subs: Records<Subscription>,
    pub(crate) apps: Records<Application>,
    pub(crate) endpoints: Records<Endpoint>,
    pub(crate) sms: Records<Sms>,
    /// The other services (SQS takes the notifications), when there are any.
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    patterns: Patterns,
}

/// What the actor answers: the reply, and the commit it must wait for.
#[derive(Debug)]
pub(crate) struct Answer {
    pub(crate) reply: Reply,
    pub(crate) durable: Option<Ack>,
}

/// The operations this service runs. The others are refused by name before their request is read: the SMS sandbox and origination numbers
/// are cut (SCOPE.md), and nothing else of the model is left out.
pub(crate) fn built(op: Operation) -> bool {
    !matches!(
        op,
        Operation::CreateSMSSandboxPhoneNumber
            | Operation::DeleteSMSSandboxPhoneNumber
            | Operation::GetSMSSandboxAccountStatus
            | Operation::ListSMSSandboxPhoneNumbers
            | Operation::VerifySMSSandboxPhoneNumber
            | Operation::ListOriginationNumbers
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
            && let Err(refusal) = peers.authorize(&call.caller, self.needs(&call)).await
        {
            return Answer {
                reply: Err(denied(refusal)),
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

/// SNS refuses as 403 `AuthorizationError` (recorded 2026-11-09), with IAM's sentence.
fn denied(refusal: ApiError) -> ApiError {
    ApiError::modeled("AuthorizationErrorException", refusal.message)
}

impl Core {
    /// SNS with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>, peers: Option<Arc<Peers>>) -> Self {
        Self::with(clock, Tables::empty(), peers, None)
    }

    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(
            dir,
            FILE,
            &[TOPICS, SUBSCRIPTIONS, APPLICATIONS, ENDPOINTS, SMS],
        )?;
        let read = db.read()?;
        let tables = Tables {
            topics: Records::load(TOPICS, "topic", VERSION, &read)?,
            subs: Records::load(SUBSCRIPTIONS, "subscription", VERSION, &read)?,
            apps: Records::load(APPLICATIONS, "application", VERSION, &read)?,
            endpoints: Records::load(ENDPOINTS, "endpoint", VERSION, &read)?,
            sms: Records::load(SMS, "SMS settings", VERSION, &read)?,
        };
        drop(read);
        Ok(Self::with(clock, tables, peers, Some(db)))
    }

    fn with(clock: Arc<dyn Clock>, t: Tables, peers: Option<Arc<Peers>>, db: Option<Db>) -> Self {
        Self {
            topics: t.topics,
            subs: t.subs,
            apps: t.apps,
            endpoints: t.endpoints,
            sms: t.sms,
            peers,
            clock,
            db,
            patterns: Patterns::new(&TABLES),
        }
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.topics.stage()?,
            self.subs.stage()?,
            self.apps.stage()?,
            self.endpoints.stage()?,
            self.sms.stage()?,
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
        let r = Req::new(call);
        let value = match op {
            Operation::Publish => self.publish(&r).await?,
            Operation::PublishBatch => self.publish_batch(&r).await?,
            other => self.run(other, &r)?,
        };
        reply_json(&TABLES, &self.patterns, call.op, &value)
    }

    fn run(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        match op {
            Operation::CreateTopic => self.create_topic(r),
            Operation::DeleteTopic => self.delete_topic(r),
            Operation::ListTopics => self.list_topics(r),
            Operation::GetTopicAttributes => self.get_topic_attributes(r),
            Operation::SetTopicAttributes => self.set_topic_attributes(r),
            Operation::AddPermission => self.add_permission(r),
            Operation::RemovePermission => self.remove_permission(r),
            Operation::TagResource => self.tag_resource(r),
            Operation::UntagResource => self.untag_resource(r),
            Operation::ListTagsForResource => self.list_tags(r),
            Operation::PutDataProtectionPolicy => self.put_data_protection_policy(r),
            Operation::GetDataProtectionPolicy => self.get_data_protection_policy(r),
            _ => self.run_subscriptions(op, r),
        }
    }

    fn run_subscriptions(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        match op {
            Operation::Subscribe => self.subscribe(r),
            Operation::ConfirmSubscription => self.confirm_subscription(r),
            Operation::Unsubscribe => self.unsubscribe(r),
            Operation::GetSubscriptionAttributes => self.get_subscription_attributes(r),
            Operation::SetSubscriptionAttributes => self.set_subscription_attributes(r),
            Operation::ListSubscriptions => self.list_subscriptions(r, false),
            Operation::ListSubscriptionsByTopic => self.list_subscriptions(r, true),
            _ => self.run_push(op, r),
        }
    }

    /// Mobile push and SMS: stored, nothing sent.
    fn run_push(&mut self, op: Operation, r: &Req<'_>) -> Result<Value, ApiError> {
        match op {
            Operation::CreatePlatformApplication => self.create_platform_application(r),
            Operation::DeletePlatformApplication => self.delete_platform_application(r),
            Operation::ListPlatformApplications => self.list_platform_applications(r),
            Operation::GetPlatformApplicationAttributes => self.get_application_attributes(r),
            Operation::SetPlatformApplicationAttributes => self.set_application_attributes(r),
            Operation::CreatePlatformEndpoint => self.create_platform_endpoint(r),
            Operation::DeleteEndpoint => self.delete_endpoint(r),
            Operation::GetEndpointAttributes => self.get_endpoint_attributes(r),
            Operation::SetEndpointAttributes => self.set_endpoint_attributes(r),
            Operation::ListEndpointsByPlatformApplication => self.list_endpoints(r),
            Operation::SetSMSAttributes => self.set_sms_attributes(r),
            Operation::GetSMSAttributes => self.get_sms_attributes(r),
            Operation::CheckIfPhoneNumberIsOptedOut => self.check_opted_out(r),
            Operation::OptInPhoneNumber => self.opt_in(r),
            Operation::ListPhoneNumbersOptedOut => Ok(self.list_opted_out()),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}

/// What `with` is given: the tables, loaded or empty.
struct Tables {
    topics: Records<Topic>,
    subs: Records<Subscription>,
    apps: Records<Application>,
    endpoints: Records<Endpoint>,
    sms: Records<Sms>,
}

impl Tables {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            topics: Records::new(TOPICS, "topic", VERSION),
            subs: Records::new(SUBSCRIPTIONS, "subscription", VERSION),
            apps: Records::new(APPLICATIONS, "application", VERSION),
            endpoints: Records::new(ENDPOINTS, "endpoint", VERSION),
            sms: Records::new(SMS, "SMS settings", VERSION),
        }
    }
}

/// The bus the host handed this service, if it did.
fn peers_of(ctx: &Ctx) -> Option<Arc<Peers>> {
    ctx.ext
        .as_ref()
        .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
}
