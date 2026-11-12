//! What a parameter may be called, and how a request names one.
//!
//! The service model gives `Name` a length and nothing else, so the rules here come from its documentation and from what AWS
//! answered when the conformance scenarios were recorded (2026-11-09). A name that breaks a character rule was answered with
//! the "can't be prefixed with ssm" text; the reserved-prefix cases were answered `AccessDenied` by the recorder's policy, so
//! their words (and whether `/awsome/x` is allowed) are not recorded.

use aws_wire::ApiError;

/// The deepest a path may go.
const MAX_LEVELS: usize = 15;

/// The words AWS used for a name with a character it does not allow (recorded).
const NAME_RULE: &str = "Parameter name: can't be prefixed with \"ssm\" (case-insensitive). If formed as a path, it can consist of sub-paths divided by slash symbol; each sub-path can be formed as a mix of letters, numbers and the following 3 symbols .-_";

/// The words for a path that is not one (unrecorded: the recorder was denied this call).
const PATH_RULE: &str = "The parameter doesn't meet the parameter name requirements. The parameter name must begin with a forward slash \"/\". It can't be prefixed with \"aws\" or \"ssm\" (case-insensitive). It must use only letters, numbers, or the following symbols: . (period), - (hyphen), _ (underscore). Special characters are not allowed. All sub-paths, if specified, must use the forward slash symbol \"/\". Valid example: /get/parameters2-/by1./path0_.";

pub(crate) fn validation(message: impl Into<String>) -> ApiError {
    ApiError::modeled("ValidationException", message)
}

fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Checks a name being written. Reads are not held to the reserved words: `/aws/service/...` is where AWS keeps its public
/// parameters.
pub(crate) fn check_write(name: &str) -> Result<(), ApiError> {
    let path = name.strip_prefix('/').unwrap_or(name);
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > MAX_LEVELS {
        return Err(ApiError::modeled(
            "HierarchyLevelLimitExceededException",
            "A hierarchy can have a maximum of 15 levels.",
        ));
    }
    if !segments.iter().all(|s| segment_ok(s)) {
        return Err(validation(NAME_RULE));
    }
    let first = segments.first().copied().unwrap_or_default();
    let lower = first.to_ascii_lowercase();
    let reserved = if name.starts_with('/') {
        lower == "aws" || lower == "ssm"
    } else {
        lower.starts_with("aws") || lower.starts_with("ssm")
    };
    if reserved {
        return Err(validation(NAME_RULE));
    }
    Ok(())
}

/// Checks a path being read: it starts with a slash and its parts are well formed. Returns it without a trailing slash.
pub(crate) fn check_path(path: &str) -> Result<&str, ApiError> {
    let Some(rest) = path.strip_prefix('/') else {
        return Err(validation(PATH_RULE));
    };
    let trimmed = path.trim_end_matches('/');
    let rest = rest.trim_end_matches('/');
    if !rest.is_empty() && !rest.split('/').all(segment_ok) {
        return Err(validation(PATH_RULE));
    }
    Ok(trimmed)
}

/// What `GetParameter` was asked for: a name, and the version or label after a colon. The name may be an ARN.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Selector<'a> {
    pub(crate) name: &'a str,
    pub(crate) qualifier: Option<&'a str>,
}

pub(crate) fn selector(text: &str) -> Selector<'_> {
    let name = match text.strip_prefix("arn:") {
        Some(_) => text.split_once(":parameter").map_or(text, |(_, rest)| rest),
        None => text,
    };
    match name.rsplit_once(':') {
        Some((name, qualifier)) if !name.is_empty() => Selector {
            name,
            qualifier: Some(qualifier),
        },
        _ => Selector {
            name,
            qualifier: None,
        },
    }
}

/// The ARN of a parameter.
pub(crate) fn arn(region: &str, account: &str, name: &str) -> String {
    let slash = if name.starts_with('/') { "" } else { "/" };
    format!("arn:aws:ssm:{region}:{account}:parameter{slash}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_held_to_the_rules() {
        assert!(check_write("/app/db-host_1.x").is_ok());
        assert!(check_write("plain").is_ok());
        assert!(check_write("/app/has space").is_err());
        assert!(check_write("/aws/x").is_err());
        assert!(check_write("/SSM/x").is_err());
        assert!(check_write("ssm-x").is_err());
        // Deeper in the path the word is just a word (recorded: AWS accepted it).
        assert!(check_write("/app/aws-inside").is_ok());
        let deep = format!("/{}", ["a"; 16].join("/"));
        assert!(check_write(&deep).is_err());
    }

    #[test]
    fn a_selector_splits_off_a_version_or_label() {
        assert_eq!(selector("/a/b").qualifier, None);
        assert_eq!(selector("/a/b:3").qualifier, Some("3"));
        assert_eq!(selector("/a/b:stable").name, "/a/b");
        let by_arn = selector("arn:aws:ssm:us-east-1:123456789012:parameter/a/b:2");
        assert_eq!((by_arn.name, by_arn.qualifier), ("/a/b", Some("2")));
    }

    #[test]
    fn the_arn_of_a_plain_name_gets_a_slash() {
        assert_eq!(arn("r", "1", "x"), "arn:aws:ssm:r:1:parameter/x");
        assert_eq!(arn("r", "1", "/x/y"), "arn:aws:ssm:r:1:parameter/x/y");
    }
}
