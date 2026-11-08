//! Request handlers and the pure state machines: groups, transactions, idempotency, ACLs, SCRAM. No sockets
//! here: the server hands a decoded request in and gets an [`Outcome`] back.

mod broker;
mod outcome;

pub use broker::Broker;
pub use outcome::Outcome;
