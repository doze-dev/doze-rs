//! The Redis protocol, as much of it as the valkey and kvrocks modules need (docs/port/modules/PLAN.md step 7): the
//! `REDIS_URL` a client is given, a RESP2 client that sends a command and reads one reply, and what the engine says
//! about itself (`INFO`, `MONITOR`) for the console.

mod client;
mod info;
mod reply;
mod url;

pub use client::Client;
pub use info::{Monitored, fields, human_bytes, parse_info, parse_monitor};
pub use reply::{ParseError, Reply, encode, parse};
pub use url::redis_url;
