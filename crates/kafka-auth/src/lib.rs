//! Authentication for the Kafka broker, with no I/O: who a client is, and the cryptography under that.
//!
//! Everything here is a pure function of its inputs, so every parser can be fuzzed and every rule tested without
//! a network. The crate has three jobs: say what a login is ([`Principal`], [`Mechanism`]), keep the users the
//! operator configured without ever being able to print a password ([`Credentials`]), and decide whether a
//! login is right ([`Authenticator`], [`plain`]), and what a principal that has logged in (or has not) may do
//! ([`authz`]). The cryptography is behind one module ([`crypto`]), so the
//! library under it can be swapped by changing one file.

pub mod authenticator;
pub mod authz;
pub mod credentials;
pub mod crypto;
pub mod plain;
pub mod principal;
pub mod scram;

pub use authenticator::{Authenticator, SetupError};
pub use credentials::{Credentials, CredentialsError, Secret};
pub use plain::Refusal;
pub use principal::{Mechanism, Principal};
