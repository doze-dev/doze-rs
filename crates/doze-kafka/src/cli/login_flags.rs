//! Who may log in and what they may do: the SASL and ACL flags as clap reads them (flattened into `Args`); `sasl.rs` and
//! `acl.rs` hold what they come to.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;

use super::{acl, sasl};
use crate::duration;

/// `--sasl-*` and `--acl-*`.
#[derive(Debug, clap::Args)]
pub(crate) struct LoginFlags {
    /// SASL mechanisms to offer, comma separated (PLAIN, SCRAM-SHA-256, SCRAM-SHA-512; a SCRAM mechanism is
    /// derived from each user's password at start). With it somebody must be able to log in: give --sasl-user,
    /// --sasl-users-file or DOZE_KAFKA_SASL_USERS, or start on a data directory that holds users made over the API.
    /// Without it, no client is asked to log in.
    #[arg(long, value_delimiter = ',')]
    sasl_mechanisms: Vec<String>,
    /// A user that may log in, as user:password (split at the first colon; repeatable). Needs
    /// --sasl-mechanisms. Passwords are ASCII and are never printed. They show in `ps`: see --sasl-users-file.
    #[arg(long, value_name = "USER:PASSWORD")]
    sasl_user: Vec<sasl::Login>,
    /// A file of users that may log in, one user:password a line (split at the first colon; blank lines and lines that
    /// start with # are skipped), so that passwords stay out of `ps`. Warns at start if group or others can read it.
    /// The variable DOZE_KAFKA_SASL_USERS (comma-separated user:password, no comma in a password) is read too. Users
    /// from these are re-applied at every start, never stored, and cannot be changed over the API.
    #[arg(long, value_name = "FILE")]
    sasl_users_file: Option<PathBuf>,
    /// How long a failed login waits before it is answered, to make guessing passwords slow (Kafka's
    /// connection.failed.authentication.delay.ms, 100ms; 0 = at once; at most 10s). Applies to every login that is
    /// refused. The connection is then closed after at most 250ms more, outside the 30s a login is given.
    #[arg(long, default_value = "100ms", value_parser = duration::failed_login_delay)]
    pub(crate) sasl_failed_login_delay: Duration,

    /// What a resource that no ACL covers allows: allow (the default, so that a broker with no ACLs refuses nothing)
    /// or deny (Kafka's own default). ACLs you create are enforced either way. With SASL on, allow means every
    /// logged-in user may do everything, including managing users and ACLs (only the credentials of --acl-super-user
    /// principals are kept from them): use deny with ACLs for a broker that is shared.
    #[arg(long, default_value = "allow")]
    acl_default: acl::AclDefault,
    /// A principal that ACLs never refuse, as Type:name such as User:admin (repeatable, or separated by commas or semicolons).
    /// Only a super user may set or delete the SCRAM credential of a super user over the API: with --acl-default allow,
    /// naming the administrators here is what stops any other login from taking their place.
    #[arg(long, value_name = "PRINCIPAL")]
    acl_super_user: Vec<String>,
}

impl LoginFlags {
    /// A resource no ACL covers is allowed (`--acl-default allow`).
    pub(super) fn acl_allow(&self) -> bool {
        self.acl_default == acl::AclDefault::Allow
    }

    /// The principals ACLs never refuse.
    pub(super) fn super_users(&self) -> Result<std::collections::BTreeSet<String>> {
        acl::super_users(&self.acl_super_user)
    }

    /// The users and the authenticator the flags (and the environment, and the data directory) come to.
    pub(super) fn sasl(
        &self,
        env: &dyn Fn(&str) -> Option<std::ffi::OsString>,
        data_dir: &Path,
    ) -> Result<sasl::Sasl> {
        sasl::setup(&sasl::Input {
            mechanisms: &self.sasl_mechanisms,
            users: &self.sasl_user,
            users_file: self.sasl_users_file.as_deref(),
            env: sasl::users_from_env(env)?,
            data_dir,
        })
    }
}
