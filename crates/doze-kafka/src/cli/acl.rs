//! The authorization flags: `--acl-default` and `--acl-super-user`, checked in the Go edition's words.

use std::collections::BTreeSet;
use std::str::FromStr;

use anyhow::{Result, bail};
use kafka_auth::authz::check_principal_shape;

/// What a resource that no ACL covers allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AclDefault {
    /// Everything: with no ACLs the broker refuses nothing. This is the default.
    Allow,
    /// Nothing: Kafka's own default.
    Deny,
}

impl FromStr for AclDefault {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "allow" => Ok(Self::Allow),
            "deny" => Ok(Self::Deny),
            other => Err(format!(
                "config: acl-default {other:?} must be \"allow\" or \"deny\""
            )),
        }
    }
}

/// The super users: each flag is one principal or a comma list, each written `Type:name`.
pub(super) fn super_users(flags: &[String]) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for principal in flags.iter().flat_map(|f| f.split(',')).map(str::trim) {
        if principal.is_empty() {
            continue;
        }
        if !check_principal_shape(principal) {
            bail!(
                "config: acl super user {principal:?} must be written Type:name, such as User:admin"
            );
        }
        out.insert(principal.to_owned());
    }
    Ok(out)
}
