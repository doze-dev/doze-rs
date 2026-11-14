//! The engine's errors, with Go's words (the CLI and the app show them as they are).

/// Why a wake, stop or verb failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No service by that name (a `fault.NotFound`).
    NotDeclared { name: String, config: String },
    /// Taken out of this run with `off`.
    Off(String),
    /// The stack is stopping for good.
    ShuttingDown(String),
    /// A readiness check ran out of time (Go's `fault.Timeout`: exit class and wire fault).
    Timeout(String),
    /// A start failed; the message says why.
    Failed(String),
}

/// The kind of a failure, as it crosses the control socket and becomes an exit class (Go's `internal/fault`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    NotFound,
    Timeout,
    /// Neither: a plain failure.
    Other,
}

impl Error {
    /// Its kind, for the wire and the exit class.
    pub fn fault(&self) -> Fault {
        match self {
            Error::NotDeclared { .. } => Fault::NotFound,
            Error::Timeout(_) => Fault::Timeout,
            _ => Fault::Other,
        }
    }

    /// Whether it is a "not found" (exit class 4 at the CLI, `not_found` on the wire).
    pub fn not_found(&self) -> bool {
        matches!(self, Error::NotDeclared { .. })
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotDeclared { name, config } => {
                write!(f, "instance \"{name}\" is not declared in {config}")
            }
            Error::Off(name) => {
                write!(
                    f,
                    "{name} is off: `doze on {name}` puts it back into this run"
                )
            }
            Error::ShuttingDown(name) => {
                write!(f, "daemon is shutting down; not booting \"{name}\"")
            }
            Error::Timeout(msg) | Error::Failed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {}

/// Why a stack could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// Two services share a name.
    Duplicate(String),
    /// A service needs one that is not declared.
    UnknownNeed { service: String, need: String },
    /// Needs go round in a circle; the names along it.
    Cycle(Vec<String>),
    /// A block asks for something that cannot work (Go refused it as the block was read).
    Invalid { service: String, message: String },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Duplicate(n) => write!(f, "\"{n}\" is declared twice"),
            BuildError::UnknownNeed { service, need } => {
                write!(
                    f,
                    "instance \"{service}\" depends on undeclared instance \"{need}\""
                )
            }
            BuildError::Cycle(path) => write!(f, "dependency cycle: {}", path.join(" → ")),
            BuildError::Invalid { service, message } => write!(f, "\"{service}\": {message}"),
        }
    }
}

impl std::error::Error for BuildError {}
