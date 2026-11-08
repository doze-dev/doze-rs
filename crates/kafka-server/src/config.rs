use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use kafka_wire::profile::{KafkaMajor, Profile};

/// How long a connection may take at each step. The defaults are Kafka's own where it has one.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// How long a connection may sit between requests (`connections.max.idle.ms`).
    pub idle: Duration,
    /// How long a request may take to arrive once its first byte has: a slow or stalled sender does not hold
    /// a connection for ever.
    pub frame: Duration,
    /// How long one response may take to write, to a client that has stopped reading.
    pub write: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(600),
            frame: Duration::from_secs(30),
            write: Duration::from_secs(30),
        }
    }
}

/// What the server listens on and how it treats a connection.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where to listen. Port 0 takes any free port ([`crate::Running::local_addr`] says which).
    pub listen: SocketAddr,
    /// The Kafka version being stood for.
    pub profile: Profile,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// The most connections served at once; the next is closed as it arrives. Far above what local development
    /// uses and far below what exhausts a default descriptor limit.
    pub max_connections: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 9092)),
            profile: Profile::new(KafkaMajor::DEFAULT),
            timeouts: Timeouts::default(),
            max_connections: 2048,
        }
    }
}
