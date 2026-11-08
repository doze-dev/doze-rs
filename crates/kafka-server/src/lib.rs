//! Listener, connection tasks and shutdown. (TLS and sessions join in Phase 8.)
//!
//! A connection is a plain loop: read a frame, split the header, decode the body, hand it to the broker,
//! encode the answer, write it. One request at a time, in order, as Kafka's own broker serves a connection:
//! clients are written for that, and a long-poll Fetch holds its connection. The loop is generic over the
//! stream, so TCP, the unix sockets of Phase 11 and the in-memory streams of the tests share it.

mod accept;
mod config;
mod connection;
mod frames;
mod pool;
mod server;

pub use config::{Config, Timeouts};
pub use server::{Running, Server};
