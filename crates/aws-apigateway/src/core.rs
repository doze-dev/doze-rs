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

use crate::doc::Doc;
use crate::plane::{Find, Found};
use crate::req::{Req, Res};
use crate::usage::Gate;

const FILE: &str = "apigateway.redb";
const REST: &str = "rest";
const HTTP: &str = "http";
const KEYS: &str = "keys";
const PLANS: &str = "plans";
const DOMAINS: &str = "domains";
const ACCOUNT: &str = "account";
const CERTS: &str = "certs";
/// VPC links of both APIs, `v1/<id>` and `v2/<id>`.
const LINKS: &str = "links";
const USAGE: &str = "usage";
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
    /// Whether an API key value opens a stage: the key's id when it exists, is enabled, is in a usage plan for the stage and the
    /// plan's quota and throttle let the request through (it is then counted).
    Key {
        value: String,
        api: String,
        stage: String,
        now_ms: i64,
    },
    /// From `doze-aws data`: what API Gateway holds, or remove it all.
    Data(Box<crate::storage::DataCall>),
}

impl std::fmt::Debug for Ask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ask")
    }
}

/// What the actor answers.
pub(crate) enum Answer {
    Reply {
        reply: Res,
        durable: Option<Ack>,
    },
    Found(Result<Box<Found>, crate::plane::Miss>),
    Key(Gate),
    /// A data question was answered on its own channel.
    Data,
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
    /// Client certificates, by id (the private key kept as `_key`).
    pub(crate) certs: Records<Doc>,
    pub(crate) links: Records<Doc>,
    /// Requests counted per usage plan and key, by day.
    pub(crate) usage: Records<Doc>,
    pub(crate) buckets: crate::usage::Buckets,
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
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
            Ask::Key {
                value,
                api,
                stage,
                now_ms,
            } => Answer::Key(self.key_gate(&value, &api, &stage, now_ms)),
            Ask::Data(asked) => {
                let crate::storage::DataCall { region, ask, reply } = *asked;
                if reply.send(self.data(&region, ask).await).is_err() {
                    // WHY: the person stopped waiting; what was asked was done all the same.
                }
                Answer::Data
            }
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
        let db = Db::open(
            dir,
            FILE,
            &[
                REST, HTTP, KEYS, PLANS, DOMAINS, ACCOUNT, CERTS, LINKS, USAGE,
            ],
        )?;
        let read = db.read()?;
        let core = Self {
            rest: Records::load(REST, "REST API", VERSION, &read)?,
            http: Records::load(HTTP, "HTTP API", VERSION, &read)?,
            keys: Records::load(KEYS, "API key", VERSION, &read)?,
            plans: Records::load(PLANS, "usage plan", VERSION, &read)?,
            domains: Records::load(DOMAINS, "domain name", VERSION, &read)?,
            account: Records::load(ACCOUNT, "account", VERSION, &read)?,
            certs: Records::load(CERTS, "client certificate", VERSION, &read)?,
            links: Records::load(LINKS, "VPC link", VERSION, &read)?,
            usage: Records::load(USAGE, "usage count", VERSION, &read)?,
            buckets: crate::usage::Buckets::default(),
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

    /// How many definitions there are, and the bytes of their rows.
    pub(crate) fn count(&self) -> (u64, u64) {
        use crate::storage::tally;
        [
            tally(&self.rest),
            tally(&self.http),
            tally(&self.keys),
            tally(&self.plans),
            tally(&self.domains),
            tally(&self.account),
            tally(&self.certs),
            tally(&self.links),
            tally(&self.usage),
        ]
        .iter()
        .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    }

    /// Removes every API, key, usage plan, domain name and the account settings: API Gateway as a fresh data directory has it.
    pub(crate) fn wipe(&mut self) -> Result<(), StoreError> {
        use crate::storage::clear;
        clear(&mut self.rest);
        clear(&mut self.http);
        clear(&mut self.keys);
        clear(&mut self.plans);
        clear(&mut self.domains);
        clear(&mut self.account);
        clear(&mut self.certs);
        clear(&mut self.links);
        clear(&mut self.usage);
        self.buckets.clear();
        drop(self.flush()?);
        Ok(())
    }

    fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.rest.stage()?,
            self.http.stage()?,
            self.keys.stage()?,
            self.plans.stage()?,
            self.domains.stage()?,
            self.account.stage()?,
            self.certs.stage()?,
            self.links.stage()?,
            self.usage.stage()?,
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

    /// A key check on the plane: counted (and so written) when it opens the stage.
    fn key_gate(&mut self, value: &str, api: &str, stage: &str, now_ms: i64) -> Gate {
        let gate = self.gate(value, api, stage, now_ms);
        if matches!(gate, Gate::Open(_)) {
            match self.flush() {
                // WHY: a count need not be on disk before the request is served; the writer commits it with the next group.
                Ok(ack) => drop(ack),
                Err(e) => tracing::warn!("API Gateway could not save a usage count: {e}"),
            }
        }
        gate
    }
}
