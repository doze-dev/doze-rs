/// The listener a connection came in on: what a client is told to connect to next. (A broker can be reached
/// at several addresses; each says its own.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// The host a client should use, for this broker.
    pub advertised_host: String,
    /// The port a client should use.
    pub advertised_port: i32,
    /// Where the client connected from, as DescribeGroups shows it: a slash, then its address.
    pub client_host: String,
    /// The connection, as the server numbers them from 1; 0 for a request that came over none (a test, the console).
    /// The share sessions a connection opens close with it ([`Broker::connection_closed`](crate::Broker)).
    pub connection: u64,
}

impl Listener {
    /// This listener, for a connection from `peer`: Kafka shows a member's address as a slash and the address.
    #[must_use]
    pub fn seen_from(&self, peer: std::net::IpAddr) -> Self {
        Self {
            client_host: format!("/{peer}"),
            ..self.clone()
        }
    }

    /// This listener, for the connection numbered `id`.
    #[must_use]
    pub fn on_connection(self, id: u64) -> Self {
        Self {
            connection: id,
            ..self
        }
    }
}
