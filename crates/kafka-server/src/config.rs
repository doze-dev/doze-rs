use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use kafka_broker::Listener;
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
    /// The host clients are told to connect to. Unset, it follows the listen address: a specific address
    /// advertises itself, and a wildcard advertises `localhost`.
    pub advertised_host: Option<String>,
    /// The port clients are told to connect to. Unset, the port actually bound (so port 0 works).
    pub advertised_port: Option<u16>,
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
            advertised_host: None,
            advertised_port: None,
            profile: Profile::new(KafkaMajor::DEFAULT),
            timeouts: Timeouts::default(),
            max_connections: 2048,
        }
    }
}

impl Config {
    /// What a client is told to connect to, given the address that was bound.
    pub(crate) fn advertise(&self, bound: SocketAddr) -> Listener {
        let host = self.advertised_host.clone().unwrap_or_else(|| {
            if bound.ip().is_unspecified() {
                "localhost".to_owned()
            } else {
                bound.ip().to_string()
            }
        });
        Listener {
            advertised_host: host,
            advertised_port: i32::from(self.advertised_port.unwrap_or(bound.port())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broker_advertises_where_it_is_unless_told_otherwise() {
        let at = |s: &str| s.parse::<SocketAddr>().unwrap();
        let config = Config::default();
        let told = |l: Listener| (l.advertised_host, l.advertised_port);
        assert_eq!(
            told(config.advertise(at("127.0.0.1:41000"))),
            ("127.0.0.1".into(), 41000)
        );
        assert_eq!(
            told(config.advertise(at("0.0.0.0:9092"))),
            ("localhost".into(), 9092)
        );
        let named = Config {
            advertised_host: Some("kafka.test".into()),
            advertised_port: Some(19092),
            ..config
        };
        assert_eq!(
            told(named.advertise(at("0.0.0.0:9092"))),
            ("kafka.test".into(), 19092)
        );
    }
}
