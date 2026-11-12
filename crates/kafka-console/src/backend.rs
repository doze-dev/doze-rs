//! The console as a Kafka client in the same process: every read of the broker and every write goes through
//! [`Broker::handle`], with the console's own principal, as a client's request does.
//!
//! The request is built here (`requests`), at the highest version the profile offers, and the answer is the same
//! struct a client would decode. The console's own requests never reach the connection loop, so they leave no row
//! on the wire (the recorder of slice 9e sees only clients).

use std::net::IpAddr;
use std::sync::Arc;

use kafka_auth::Principal;
use kafka_broker::{Broker, Caller, Instance, Listener, Outcome};
use kafka_storage::fs::RealFs;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::header::RequestHead;
use kafka_wire::profile::Profile;

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
        }
    }

    /// The same handle acting as another caller. For the tests that show a refusal as a client would meet it: the console
    /// itself is a super user, so nothing the console does is ever refused. Not part of a release build.
    #[cfg(any(test, feature = "examples"))]
    #[must_use]
    pub fn acting_as(mut self, caller: Caller) -> Self {
        self.caller = caller;
        self
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

    /// The facts of this run that the protocol does not say.
    #[must_use]
    pub fn instance(&self) -> Option<Instance> {
        self.broker.console_view().instance(&self.caller)
    }
}
