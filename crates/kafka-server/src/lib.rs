//! Listener, TLS (the `tls` feature), connection tasks, login sessions and shutdown.
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
mod connections;
pub mod fault;
mod frames;
mod observer;
mod pool;
mod server;
mod session;
#[cfg(feature = "tls")]
mod tls;

pub use config::{Config, MOST_FAILED_LOGIN_DELAY, Timeouts};
pub use observer::{Answer, Asked, Call, ConnId, Observed, Observer, Opened, Pending};
pub use server::{Running, Server};
#[cfg(feature = "tls")]
pub use tls::{TlsConfig, TlsError};
