//! The console as a Kafka client in the same process: every read of the broker and every write goes through
//! [`Broker::handle`], with the console's own principal, as a client's request does.
//!
//! The request is built here (`requests`), at the highest version the profile offers, and the answer is the same
//! struct a client would decode. The console's own requests never reach the connection loop, so they leave no row
//! on the wire (the recorder of slice 9e sees only clients).

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Instance, Listener, Outcome};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::profile::Profile;

mod config;
mod forms;
mod log_views;
pub mod requests;

/// The client id the console's requests carry.
pub const CLIENT_ID: &str = "console";
/// The correlation id of every request: nothing is matched on it, the call is awaited.
const CORRELATION_ID: i32 = 1;

/// Why a call did not come back with an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallError {
    /// The broker answers nothing to this request (an API it does not serve).
    NoAnswer,
}

/// The broker, as the console reaches it.
#[derive(Debug, Clone)]
pub struct Backend {
    broker: Arc<Broker<RealFs>>,
    caller: Caller,
    listener: Listener,
    profile: Profile,
    /// The APIs called, in order, when a test asks to see them (see [`Backend::recording`]); none in a real console.
    spy: Option<Arc<Mutex<Vec<kafka_wire::messages::ApiKey>>>>,
}

impl Backend {
    /// A handle on `broker`: calls are made as `User:__console` from the loopback address, on `listener` (what clients are
    /// told), speaking the `profile` of the Kafka version being stood for.
    #[must_use]
    pub fn new(broker: Arc<Broker<RealFs>>, listener: Listener, profile: Profile) -> Self {
        let caller = Caller::new(Principal::console(), Some(IpAddr::from([127, 0, 0, 1])));
        Self {
            broker,
            caller,
            listener,
            profile,
            spy: None,
        }
    }

    /// The same handle, but calls are made as `caller` and not as the console. A test seam: the console is a super user of
    /// every broker (SEC-241), so this is how a page is shown a refusal for want of an ACL, as a client would meet it.
    #[doc(hidden)]
    #[must_use]
    pub fn acting_as(&self, caller: Caller) -> Self {
        Self {
            caller,
            ..self.clone()
        }
    }

    /// The same handle with a log of every API it calls. A test seam: it proves that a page that only reads never writes.
    #[doc(hidden)]
    #[must_use]
    pub fn recording(&self) -> (Self, Arc<Mutex<Vec<kafka_wire::messages::ApiKey>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                spy: Some(Arc::clone(&log)),
                ..self.clone()
            },
            log,
        )
    }

    /// The caller of every call: the console's principal.
    #[must_use]
    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    /// The highest version of an API the profile offers.
    #[must_use]
    pub fn version(&self, key: kafka_wire::messages::ApiKey) -> i16 {
        self.profile.range(key).map_or(0, |(_, highest)| highest)
    }

    /// Puts `request` to the broker, at the highest version, and waits for the answer.
    ///
    /// # Errors
    /// [`CallError::NoAnswer`] if the broker answers nothing (it does not serve the API).
    pub async fn call(&self, request: Request) -> Result<Response, CallError> {
        let key = request.api_key();
        if let Some(log) = &self.spy {
            log.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(key);
        }
        let head = RequestHead {
            key,
            version: self.version(key),
            correlation_id: CORRELATION_ID,
            client_id: Some(StrBytes::from_static_str(CLIENT_ID)),
        };
        match self
            .broker
            .handle(&self.listener, &self.caller, &head, request)
            .await
        {
            Outcome::Respond(response) => Ok(response),
            _ => Err(CallError::NoAnswer),
        }
    }

    /// The files of each partition of `topic` (size, last write, offline cause), if the console may describe it.
    #[must_use]
    pub fn partition_files(&self, topic: &str) -> Option<Vec<kafka_broker::PartitionFiles>> {
        self.broker
            .console_view()
            .partition_files(&self.caller, topic)
    }

    /// What the protocol hides about one group (leader, timeouts, last heartbeat, when the sweep takes its commits), if the
    /// console may describe it.
    pub async fn group_detail(&self, group: &str) -> Option<kafka_broker::GroupDetail> {
        self.broker
            .console_view()
            .group_detail(&self.caller, group)
            .await
    }

    /// The facts of this run that the protocol does not say.
    #[must_use]
    pub fn instance(&self) -> Option<Instance> {
        self.broker.console_view().instance(&self.caller)
    }
}
