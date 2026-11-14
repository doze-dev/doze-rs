//! What the binary knows of how the broker was started that the Settings and Access pages say: the flags with their values
//! and defaults, where the console listens, which certificate TLS was given, and how long a failed login waits. The pages
//! work without it (a broker started by a test has none): they say less, and never invent it.

use std::time::Duration;

use crate::model::SettingsFlag;

/// How the broker was started, beyond [`crate::Facts`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartFacts {
    /// Every flag, in the order the table lists them. Empty where the flags are not known.
    pub flags: Vec<SettingsFlag>,
    /// Where the console listens, as `host:port`.
    pub console_addr: Option<String>,
    /// `--tls-cert`: the certificate's file. Never the key's file or contents.
    pub tls_cert: Option<String>,
    /// `--sasl-failed-login-delay`.
    pub failed_login_delay: Option<Duration>,
    /// `--console-unprotected` was given.
    pub console_unprotected: bool,
}

impl StartFacts {
    /// The flag called `name` (`--segment-bytes`), if the flags are known.
    #[must_use]
    pub fn flag(&self, name: &str) -> Option<&SettingsFlag> {
        self.flags.iter().find(|f| f.name == name)
    }

    /// Its value in force, if known.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&str> {
        self.flag(name).map(|f| f.value.as_str())
    }
}
