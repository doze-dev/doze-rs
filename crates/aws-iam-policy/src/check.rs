//! The rules IAM applies to a document when it is stored, and the words it answers with.
//!
//! Recorded 2026-11-09: a trust policy that is not JSON ("This policy contains invalid Json"), an identity policy that is not JSON
//! ("Syntax errors in policy."), a trust policy that names a resource ("Has prohibited field Resource"), an identity policy that
//! names a principal ("Policy document should not specify a principal."). The other messages are from memory and marked.

use crate::document::{Document, Effect, parse};

/// Which kind of document is being stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Attached to a user, group or role (or a managed policy).
    Identity,
    /// A role's `AssumeRolePolicyDocument`.
    Trust,
    /// On a resource.
    Resource,
}

/// Why a document was refused: IAM's `MalformedPolicyDocument`, with its words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyError(pub String);

fn refuse(message: impl Into<String>) -> Result<Document, PolicyError> {
    Err(PolicyError(message.into()))
}

/// Reads and checks `text` as a document of `kind`.
///
/// # Errors
/// The message IAM gives for the first thing wrong with it.
pub fn check(text: &str, kind: Kind) -> Result<Document, PolicyError> {
    let Some(parsed) = parse(text) else {
        return refuse(match kind {
            Kind::Trust => "This policy contains invalid Json",
            _ => "Syntax errors in policy.",
        });
    };
    let doc = match parsed {
        Ok(doc) => doc,
        Err(_) => return refuse("Syntax errors in policy."),
    };
    // (memory)
    if let Some(v) = &doc.version
        && !matches!(v.as_str(), "2012-10-17" | "2008-10-17")
    {
        return refuse("The policy must contain a valid version string");
    }
    if doc.statements.is_empty() {
        return refuse("Syntax errors in policy.");
    }
    for s in &doc.statements {
        match kind {
            Kind::Trust => {
                if s.resources.is_some() {
                    return refuse("Has prohibited field Resource");
                }
                if s.principal.is_none() {
                    // (memory)
                    return refuse("Missing required field Principal");
                }
                if s.actions.is_none() {
                    // (memory)
                    return refuse("Policy statement must contain actions.");
                }
            }
            Kind::Identity => {
                if s.principal.is_some() {
                    return refuse("Policy document should not specify a principal.");
                }
                if s.actions.is_none() {
                    // (memory)
                    return refuse("Policy statement must contain actions.");
                }
                if s.resources.is_none() {
                    // (memory)
                    return refuse("Policy statement must contain resources.");
                }
            }
            Kind::Resource => {
                if s.principal.is_none() {
                    // (memory)
                    return refuse("Policy statement must contain principals.");
                }
            }
        }
        let _ = s.effect == Effect::Allow;
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRUST: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"lambda.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
    const READ: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s3:GetObject","Resource":"*"}]}"#;

    fn why(text: &str, kind: Kind) -> String {
        check(text, kind).unwrap_err().0
    }

    #[test]
    fn documents_of_each_kind_are_accepted() {
        assert!(check(TRUST, Kind::Trust).is_ok());
        assert!(check(READ, Kind::Identity).is_ok());
        let one = r#"{"Statement":{"Effect":"Deny","NotAction":"s3:*","NotResource":"arn:aws:s3:::x","Condition":{"Bool":{"aws:SecureTransport":false}}}}"#;
        let doc = check(one, Kind::Identity).unwrap();
        assert_eq!(doc.statements.first().map(|s| s.effect), Some(Effect::Deny));
        assert_eq!(doc.statements.first().map(|s| s.conditions.len()), Some(1));
    }

    #[test]
    fn what_iam_answered_is_what_is_answered() {
        assert_eq!(
            why("{not json", Kind::Trust),
            "This policy contains invalid Json"
        );
        assert_eq!(why("{not json", Kind::Identity), "Syntax errors in policy.");
        assert_eq!(why(READ, Kind::Trust), "Has prohibited field Resource");
        assert_eq!(
            why(TRUST, Kind::Identity),
            "Policy document should not specify a principal."
        );
    }

    #[test]
    fn structure_faults_are_refused() {
        for bad in [
            r#"[]"#,
            r#"{"Statement":[]}"#,
            r#"{"Statement":[{"Effect":"Maybe","Action":"*","Resource":"*"}]}"#,
            r#"{"Version":"1999","Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#,
        ] {
            assert!(check(bad, Kind::Identity).is_err(), "{bad}");
        }
    }
}
