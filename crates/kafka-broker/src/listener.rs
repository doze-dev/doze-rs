/// The listener a connection came in on: what a client is told to connect to next. (A broker can be reached
/// at several addresses; each says its own.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    /// The host a client should use, for this broker.
    pub advertised_host: String,
    /// The port a client should use.
    pub advertised_port: i32,
}
