//! The Redis protocol, as much of it as the valkey and kvrocks modules need (docs/port/modules/PLAN.md step 7): the
//! `REDIS_URL` a client is given, and a RESP2 client that sends a command and reads one reply, for acceptance tests and
//! anything a module later asks its engine.

mod client;
mod reply;
mod url;

pub use client::Client;
pub use reply::{ParseError, Reply, encode, parse};
pub use url::redis_url;
