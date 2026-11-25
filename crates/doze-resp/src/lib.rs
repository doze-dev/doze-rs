//! The Redis protocol, as much of it as the valkey and kvrocks modules need (docs/port/modules/PLAN.md step 7): the
//! `REDIS_URL` a client is given, a RESP2 client that sends a command and reads one reply, what the engine says
//! about itself (`INFO`, `MONITOR`) for the console, and the console's key browser both modules share: a tree of keys
//! split on `:` ([`browse`]), each key's value by its type ([`value`]), and a form that sets a key.

pub mod browse;
mod client;
mod info;
pub mod json;
pub mod keyspace;
pub mod layout;
mod reply;
mod url;
pub mod value;

pub use client::Client;
pub use info::{Monitored, fields, human_bytes, parse_info, parse_monitor};
pub use reply::{ParseError, Reply, encode, encode_bytes, parse};
pub use url::redis_url;
