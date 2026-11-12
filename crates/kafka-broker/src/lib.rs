//! Request handlers and the state they keep: topics, partitions and their writers, producer ids, groups,
//! transactions, the ACLs that say who may do what, and the SCRAM credentials stored over the API. No sockets here: the server hands a decoded request in and gets an [`Outcome`] back.

mod authorizer;
mod blocking;
mod broker;
mod caller;
mod cleaner;
mod clock;
mod config;
mod configs;
mod error;
mod groups;
mod handlers;
mod listener;
mod logs;
mod outcome;
mod partitions;
mod producers;
mod retention;
mod topic_config;
mod topics;
mod txn;
mod txn_index;
mod users;

pub use broker::Broker;
pub use caller::Caller;
pub use config::{Config, Defaults};
pub(crate) use configs::Live;
pub use error::BrokerError;
pub use listener::Listener;
pub use outcome::Outcome;
