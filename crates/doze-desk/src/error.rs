//! Why the hub could not do what was asked.

/// A verb or an action the hub could not send, or that failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The stack has no daemon (Go `ErrNotRunning`).
    #[error("the stack is not running")]
    NotRunning,
    /// The daemon is not answering, so nothing was sent: dialling would hang and then say something worse (Go
    /// `ErrNotResponding`).
    #[error("the daemon is not answering: nothing was sent")]
    NotResponding,
    /// The daemon answered with an error, or the connection failed.
    #[error(transparent)]
    Control(#[from] doze_control::Error),
}
