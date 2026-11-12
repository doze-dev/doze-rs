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

/// The super users: each flag is one principal or a list, each written `Type:name`. The separator is a comma (the Go
/// edition's) or a semicolon (Kafka's `super.users`): taken as one principal, `User:a;User:b` would name a user
/// called `a;User:b` and leave nobody a super user (SEC-D85).
pub(super) fn super_users(flags: &[String]) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for principal in flags
        .iter()
        .flat_map(|f| f.split([',', ';']))
        .map(str::trim)
    {
        if principal.is_empty() {
            continue;
        }
        if !check_principal_shape(principal) {
            bail!(
                "config: acl super user {principal:?} must be written Type:name, such as User:admin"
            );
        }
        // The console's principal is a super user of every broker already, and naming it here would only make it look
        // like something an operator controls (SEC-240).
        if kafka_auth::Principal::is_console(principal) {
            bail!(
                "config: acl super user {principal:?} is reserved: the web console's own principal is always a super user of the broker, and cannot be named here"
            );
        }
        out.insert(principal.to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn users(flags: &[&str]) -> Result<Vec<String>> {
        let flags: Vec<String> = flags.iter().map(|f| (*f).to_owned()).collect();
        super_users(&flags).map(|s| s.into_iter().collect())
    }

    #[test]
    fn a_list_may_be_separated_by_commas_or_semicolons() {
        let want = ["User:a", "User:b"];
        assert_eq!(users(&["User:a,User:b"]).unwrap(), want);
        assert_eq!(users(&["User:a;User:b"]).unwrap(), want);
        assert_eq!(users(&["User:a; User:b ,User:a"]).unwrap(), want);
        // Both in one flag, and across repeated flags.
        assert_eq!(users(&["User:a;User:b,User:c", "User:d"]).unwrap().len(), 4);
        assert_eq!(users(&["User:a", "User:b;User:a"]).unwrap(), want);
        // Empty parts are skipped as before.
        assert_eq!(users(&[";User:a,;"]).unwrap(), ["User:a"]);
    }

    #[test]
    fn the_consoles_own_principal_cannot_be_named_as_a_super_user() {
        for flag in [
            "User:__console",
            "User:a,User:__console",
            "User:a;User:__console",
        ] {
            let message = users(&[flag]).unwrap_err().to_string();
            assert!(message.contains("is reserved"), "{flag}: {message}");
        }
        // Another name that merely looks like it is an ordinary one.
        assert_eq!(users(&["User:__console2"]).unwrap(), ["User:__console2"]);
        assert_eq!(users(&["Group:__console"]).unwrap(), ["Group:__console"]);
    }

    #[test]
    fn every_part_of_a_semicolon_list_is_checked_for_type_and_name() {
        for bad in ["User:a;admin", "admin;User:a", "User:a;User:", "User:a;:x"] {
            let message = users(&[bad]).unwrap_err().to_string();
            assert!(
                message.contains("must be written Type:name"),
                "{bad}: {message}"
            );
        }
    }
}
