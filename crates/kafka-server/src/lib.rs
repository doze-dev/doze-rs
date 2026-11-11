//! Listener, connection tasks, login sessions and shutdown. (TLS joins in Phase 8.)
//!
//! A connection is a plain loop: read a frame, split the header, decode the body, hand it to the broker,
//! encode the answer, write it. One request at a time, in order, as Kafka's own broker serves a connection:
//! clients are written for that, and a long-poll Fetch holds its connection. The loop is generic over the
//! stream, so TCP, the unix sockets of Phase 11 and the in-memory streams of the tests share it.
//!
//! With SASL configured a connection starts as a stranger: `session` says, from a request's header alone,
//! whether it may be served, and the decoder holds it to a small frame until it has logged in.

mod accept;
mod config;
mod connection;
mod frames;
mod pool;
mod server;
mod session;

pub use config::{Config, Timeouts};
pub use server::{Running, Server};
