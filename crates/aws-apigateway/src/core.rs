//! The state one region owns, behind one actor: REST APIs (v1), HTTP and WebSocket APIs (v2), API keys, usage plans, custom domain
//! names (shared by both APIs, as on AWS) and the account settings. The control plane runs here; the execute-api plane asks it to
//! find what a request reaches ([`Ask::Find`]) and does the rest outside, because an integration can take seconds.

use std::path::Path;
use std::sync::Arc;

use aws_peers::{Mode, Peers};
use aws_wire::ApiError;
use aws_wire::service::Call;
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::doc::Doc;
use crate::plane::{Find, Found};
use crate::req::{Req, Res};

const FILE: &str = "apigateway.redb";
const REST: &str = "rest";
const HTTP: &str = "http";
const KEYS: &str = "keys";
const PLANS: &str = "plans";
const DOMAINS: &str = "domains";
const ACCOUNT: &str = "account";
/// The version of every record. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// Which API a call is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Api {
    V1,
    V2,
}

/// What the actor is asked.
pub(crate) enum Ask {
    /// A control-plane operation.
    Call(Api, Box<Call>),
    /// What an execute-api request reaches.
    Find(Box<Find>),
    /// Whether an API key value opens a stage: the key's id when it exists, is enabled and is in a usage plan for the stage.
    Key {
        value: String,
        api: String,
        stage: String,
    },
}

impl std::fmt::Debug for Ask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ask")
    }
}

/// What the actor answers.
pub(crate) enum Answer {
    Reply { reply: Res, durable: Option<Ack> },
    Found(Result<Box<Found>, crate::plane::Miss>),
    Key(Option<String>),
}

impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer")
    }
}

/// One region of API Gateway. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) rest: Records<Doc>,
    pub(crate) http: Records<Doc>,
    pub(crate) keys: Records<Doc>,
    pub(crate) plans: Records<Doc>,
    pub(crate) domains: Records<Doc>,
    pub(crate) account: Records<Doc>,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    db: Option<Db>,
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let peers = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok());
        Self::open_in(&ctx.dir, Arc::clone(&ctx.clock), peers)
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        match ask {
            Ask::Call(api, call) => self.call(api, *call).await,
            Ask::Find(find) => Answer::Found(self.find(&find).map(Box::new)),
            Ask::Key { value, api, stage } => Answer::Key(self.key_opens(&value, &api, &stage)),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    pub(crate) fn open_in(
        dir: &Path,
        clock: Arc<dyn Clock>,
        peers: Option<Arc<Peers>>,
    ) -> Result<Self, StoreError> {
        let db = Db::open(dir, FILE, &[REST, HTTP, KEYS, PLANS, DOMAINS, ACCOUNT])?;
        let read = db.read()?;
        let core = Self {
            rest: Records::load(REST, "REST API", VERSION, &read)?,
            http: Records::load(HTTP, "HTTP API", VERSION, &read)?,
            keys: Records::load(KEYS, "API key", VERSION, &read)?,
            plans: Records::load(PLANS, "usage plan", VERSION, &read)?,
            domains: Records::load(DOMAINS, "domain name", VERSION, &read)?,
            account: Records::load(ACCOUNT, "account", VERSION, &read)?,
            peers,
            clock,
            db: None,
        };
        drop(read);
        Ok(Self {
            db: Some(db),
            ..core
        })
    }

    fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.rest.stage()?,
            self.http.stage()?,
            self.keys.stage()?,
            self.plans.stage()?,
            self.domains.stage()?,
            self.account.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    async fn call(&mut self, api: Api, call: Call) -> Answer {
        let r = Req::new(&call, self.clock.now_ms());
        if let Some(peers) = self.peers.clone()
            && peers.mode() != Mode::Off
        {
            let needs = crate::authz::needs(api, &call, &r, self);
            if let Err(refusal) = peers.authorize(&call.caller, needs).await {
                return Answer::Reply {
                    reply: Err(ApiError::new(403, "AccessDeniedException", refusal.message)),
                    durable: None,
                };
            }
        }
        let mut reply = match api {
            Api::V1 => crate::v1::dispatch(self, call.op, &r),
            Api::V2 => crate::v2::dispatch(self, call.op, &r),
        };
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer::Reply { reply, durable }
    }

    /// The id of the API key whose value is `value`, when it is enabled and a usage plan puts it on `api`'s `stage`.
    fn key_opens(&self, value: &str, api: &str, stage: &str) -> Option<String> {
        let (id, _) = self.keys.iter().find(|(_, k)| {
            crate::doc::text(&k.0, "value") == Some(value)
                && k.0.get("enabled").and_then(Value::as_bool) == Some(true)
        })?;
        let covered = self.plans.iter().any(|(_, p)| {
            let has_key =
                p.0.get("_keys")
                    .and_then(Value::as_array)
                    .is_some_and(|ks| ks.iter().any(|k| k.as_str() == Some(id)));
            let has_stage =
                p.0.get("apiStages")
                    .and_then(Value::as_array)
                    .is_some_and(|ss| {
                        ss.iter().any(|s| {
                            crate::doc::text(s, "apiId") == Some(api)
                                && crate::doc::text(s, "stage") == Some(stage)
                        })
                    });
            has_key && has_stage
        });
        covered.then(|| id.to_owned())
    }
}
