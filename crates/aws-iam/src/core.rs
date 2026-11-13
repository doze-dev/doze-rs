//! The state IAM and STS share (one account's identities), and the dispatch from an operation to the code that does it. IAM and STS
//! are two services on the wire, with two models and two signing names, and one body of state: a role session is made by STS from
//! what IAM holds, and an access key is IAM's to make and STS to be asked about.

use std::path::Path;
use std::sync::Arc;

use aws_peers::{Mode, Need, Peers};
use aws_schema::patterns::Patterns;
use aws_wire::ApiError;
use aws_wire::handler::reply_json;
use aws_wire::service::{Call, Caller, Reply};
use doze_runtime::{Clock, Ctx};
use doze_store::records::Records;
use doze_store::{Ack, Db, StoreError};
use serde_json::Value;

use crate::accesslog::Seen;
use crate::model::{AccessKey, Account, Entity, Kind, Managed, Profile, Provider};
use crate::req::Req;

const FILE: &str = "iam.redb";
const ENTITIES: &str = "entities";
const POLICIES: &str = "policies";
const KEYS: &str = "keys";
const PROFILES: &str = "profiles";
const PROVIDERS: &str = "providers";
const ACCOUNT: &str = "account";
const THE_ACCOUNT: &str = "account";
/// The version of every record here. A change of a record's shape is a new version with a branch that reads the old one.
const VERSION: u8 = 1;

/// Which of the two services a call is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    Iam,
    Sts,
}

/// What the actor is asked.
#[derive(Debug)]
pub(crate) enum Ask {
    /// A request for IAM or STS.
    Call { which: Which, call: Call },
    /// A service asks whether a caller may do what a request asks.
    Authorize { caller: Caller, needs: Vec<Need> },
    /// The gateway asks for the credential behind an access key id, to check a signature.
    Credential {
        access_key_id: String,
        session_token: Option<String>,
    },
}

/// What the actor answers. A call and a question both come back with the authorization questions they raised, for the access log.
#[derive(Debug)]
pub(crate) enum Answer {
    /// The reply, and the commit it must wait for.
    Call {
        reply: Reply,
        durable: Option<Ack>,
        seen: Vec<Seen>,
    },
    /// Yes, or the refusal.
    Authorize(Result<(), ApiError>, Vec<Seen>),
    /// The credential, if IAM issued the key.
    Credential(Option<aws_auth::Credential>),
}

/// All of the account's identities. Owned by one task, so no operation needs a lock.
#[derive(Debug)]
pub(crate) struct Core {
    pub(crate) entities: Records<Entity>,
    pub(crate) policies: Records<Managed>,
    pub(crate) keys: Records<AccessKey>,
    pub(crate) profiles: Records<Profile>,
    pub(crate) providers: Records<Provider>,
    account: Records<Account>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) db: Option<Db>,
    pub(crate) mode: Mode,
    iam: Patterns,
    sts: Patterns,
    /// The authorization questions answered since the last answer went out: handed to the access log with it.
    pub(crate) seen: Vec<Seen>,
}

impl doze_runtime::Service for Core {
    type Request = Ask;
    type Response = Answer;
    type Error = StoreError;

    async fn open(ctx: &Ctx, _name: &'static str) -> Result<Self, StoreError> {
        let mut core = Self::open_in(&ctx.dir, Arc::clone(&ctx.clock))?;
        if let Some(peers) = ctx
            .ext
            .as_ref()
            .and_then(|e| Arc::clone(e).downcast::<Peers>().ok())
        {
            core.mode = peers.mode();
        }
        Ok(core)
    }

    async fn handle(&mut self, ask: Ask) -> Answer {
        let (which, call) = match ask {
            Ask::Authorize { caller, needs } => {
                let verdict = self.authorize(&caller, &needs);
                // A last use noted on the way is written without anyone waiting for it: losing one to a crash costs a timestamp.
                if let Err(e) = self.flush() {
                    tracing::warn!("iam: could not save when a credential was last used: {e}");
                }
                return Answer::Authorize(verdict, std::mem::take(&mut self.seen));
            }
            Ask::Credential {
                access_key_id,
                session_token,
            } => {
                return Answer::Credential(
                    self.credential(&access_key_id, session_token.as_deref()),
                );
            }
            Ask::Call { which, call } => (which, call),
        };
        let mut reply = self.dispatch(which, &call);
        let durable = match self.flush() {
            Ok(ack) => ack,
            Err(e) => {
                reply = Err(ApiError::internal(format!("could not save: {e}")));
                None
            }
        };
        Answer::Call {
            reply,
            durable,
            seen: std::mem::take(&mut self.seen),
        }
    }

    async fn close(self) {
        if let Some(db) = self.db {
            db.close();
        }
    }
}

impl Core {
    /// IAM with no disk (the rules alone are under test).
    #[cfg(test)]
    pub(crate) fn in_memory(clock: Arc<dyn Clock>) -> Self {
        let mut core = Self::with(clock, Tables::empty(), None);
        core.ensure_account();
        core
    }

    pub(crate) fn open_in(dir: &Path, clock: Arc<dyn Clock>) -> Result<Self, StoreError> {
        let db = Db::open(
            dir,
            FILE,
            &[ENTITIES, POLICIES, KEYS, PROFILES, PROVIDERS, ACCOUNT],
        )?;
        let read = db.read()?;
        let tables = Tables {
            entities: Records::load(ENTITIES, "entity", VERSION, &read)?,
            policies: Records::load(POLICIES, "policy", VERSION, &read)?,
            keys: Records::load(KEYS, "access key", VERSION, &read)?,
            profiles: Records::load(PROFILES, "instance profile", VERSION, &read)?,
            providers: Records::load(PROVIDERS, "provider", VERSION, &read)?,
            account: Records::load(ACCOUNT, "account", VERSION, &read)?,
        };
        drop(read);
        let mut core = Self::with(clock, tables, Some(db));
        core.ensure_account();
        core.fill_aws_managed();
        Ok(core)
    }

    fn with(clock: Arc<dyn Clock>, t: Tables, db: Option<Db>) -> Self {
        Self {
            entities: t.entities,
            policies: t.policies,
            keys: t.keys,
            profiles: t.profiles,
            providers: t.providers,
            account: t.account,
            clock,
            db,
            mode: Mode::default(),
            iam: Patterns::new(&aws_model_iam::tables::TABLES),
            sts: Patterns::new(&aws_model_sts::tables::TABLES),
            seen: Vec::new(),
        }
    }

    /// The account record, with its session-signing key made the first time.
    fn ensure_account(&mut self) {
        if self.account.get(THE_ACCOUNT).is_none() {
            let signing_key = aws_seal::new_key().unwrap_or_default();
            self.account.insert(
                THE_ACCOUNT.to_owned(),
                Account {
                    signing_key,
                    ..Account::default()
                },
            );
        }
    }

    pub(crate) fn account(&self) -> Account {
        self.account.get(THE_ACCOUNT).cloned().unwrap_or_default()
    }

    pub(crate) fn account_mut(&mut self) -> Option<&mut Account> {
        self.account.get_mut(THE_ACCOUNT)
    }

    pub(crate) fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }

    /// Sends what the last call changed to the writer; the [`Ack`] resolves when it is on disk.
    pub(crate) fn flush(&mut self) -> Result<Option<Ack>, StoreError> {
        let changes = vec![
            self.entities.stage()?,
            self.policies.stage()?,
            self.keys.stage()?,
            self.profiles.stage()?,
            self.providers.stage()?,
            self.account.stage()?,
        ];
        Ok(self.db.as_ref().and_then(|db| db.submit(changes)))
    }

    /// Runs the call and writes the answer as the operation's output.
    pub(crate) fn dispatch(&mut self, which: Which, call: &Call) -> Reply {
        if self.mode != Mode::Off {
            // The Query services refuse as 403 `AccessDenied`.
            let needs = self.needs_of(which, call);
            self.authorize(&call.caller, &needs).map_err(|mut denied| {
                denied.status = 403;
                denied.code = "AccessDenied".into();
                denied.shape = None;
                denied
            })?;
        }
        let r = Req::new(call);
        let value: Value = match which {
            Which::Iam => self.run_iam(call.op, &r)?,
            Which::Sts => self.run_sts(call.op, &r)?,
        };
        match which {
            Which::Iam => reply_json(&aws_model_iam::tables::TABLES, &self.iam, call.op, &value),
            Which::Sts => reply_json(&aws_model_sts::tables::TABLES, &self.sts, call.op, &value),
        }
    }

    /// The entity of kind `kind` called `name`.
    pub(crate) fn entity(&self, kind: Kind, name: &str) -> Result<&Entity, ApiError> {
        self.entities
            .get(&entity_key(kind, name))
            .ok_or_else(|| crate::errors::no_such(kind.word(), name))
    }

    pub(crate) fn entity_mut(&mut self, kind: Kind, name: &str) -> Result<&mut Entity, ApiError> {
        self.entities
            .get_mut(&entity_key(kind, name))
            .ok_or_else(|| crate::errors::no_such(kind.word(), name))
    }
}

/// What `with` is given: the tables, loaded or empty.
struct Tables {
    entities: Records<Entity>,
    policies: Records<Managed>,
    keys: Records<AccessKey>,
    profiles: Records<Profile>,
    providers: Records<Provider>,
    account: Records<Account>,
}

impl Tables {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            entities: Records::new(ENTITIES, "entity", VERSION),
            policies: Records::new(POLICIES, "policy", VERSION),
            keys: Records::new(KEYS, "access key", VERSION),
            profiles: Records::new(PROFILES, "instance profile", VERSION),
            providers: Records::new(PROVIDERS, "provider", VERSION),
            account: Records::new(ACCOUNT, "account", VERSION),
        }
    }
}

/// The key of an entity record.
pub(crate) fn entity_key(kind: Kind, name: &str) -> String {
    format!("{}/{name}", kind.word())
}
