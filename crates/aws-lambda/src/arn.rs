//! Function references (`name`, `name:qualifier`, a full or partial ARN) and the ARNs Lambda makes.

use aws_wire::ApiError;

use crate::errors::invalid;

/// What a caller named: a function and perhaps a version or alias.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FnRef {
    pub(crate) name: String,
    pub(crate) qualifier: Option<String>,
}

/// Reads a `FunctionName` as the API accepts it. `qualifier` is the separate `Qualifier` parameter, which must agree with one in the
/// name.
///
/// # Errors
/// An ARN of another region or account, or a name that is not one of the forms.
pub(crate) fn parse(
    text: &str,
    qualifier: Option<&str>,
    region: &str,
    account: &str,
) -> Result<FnRef, ApiError> {
    let parts: Vec<&str> = text.split(':').collect();
    let (name, inline) = match parts.as_slice() {
        [name] => (*name, None),
        [name, q] => (*name, Some(*q)),
        ["arn", _, "lambda", r, a, "function", name] => {
            check(r, a, region, account)?;
            (*name, None)
        }
        ["arn", _, "lambda", r, a, "function", name, q] => {
            check(r, a, region, account)?;
            (*name, Some(*q))
        }
        [a, "function", name] => {
            check(region, a, region, account)?;
            (*name, None)
        }
        [a, "function", name, q] => {
            check(region, a, region, account)?;
            (*name, Some(*q))
        }
        _ => return Err(invalid(format!("Cannot find a function named: {text}"))),
    };
    let qualifier = match (inline, qualifier) {
        (Some(a), Some(b)) if a != b => {
            return Err(invalid(
                "The derived qualifier from the function name does not match the specified qualifier.",
            ));
        }
        (Some(q), _) | (None, Some(q)) => Some(q.to_owned()),
        (None, None) => None,
    };
    Ok(FnRef {
        name: name.to_owned(),
        qualifier: qualifier.filter(|q| !q.is_empty()),
    })
}

fn check(arn_region: &str, arn_account: &str, region: &str, account: &str) -> Result<(), ApiError> {
    if arn_region != region {
        return Err(invalid(format!(
            "Functions from '{arn_region}' are not reachable in this region ('{region}')"
        )));
    }
    if arn_account != account {
        return Err(invalid(format!(
            "Cross-account function references are not supported ('{arn_account}')."
        )));
    }
    Ok(())
}

/// `arn:aws:lambda:<region>:<account>:function:<name>[:<qualifier>]`.
pub(crate) fn function(region: &str, account: &str, name: &str, qualifier: Option<&str>) -> String {
    let base = format!("arn:aws:lambda:{region}:{account}:function:{name}");
    match qualifier {
        Some(q) if !q.is_empty() => format!("{base}:{q}"),
        _ => base,
    }
}

/// `arn:aws:lambda:<region>:<account>:layer:<name>[:<version>]`.
pub(crate) fn layer(region: &str, account: &str, name: &str, version: Option<u32>) -> String {
    let base = format!("arn:aws:lambda:{region}:{account}:layer:{name}");
    version.map_or(base.clone(), |v| format!("{base}:{v}"))
}

pub(crate) fn mapping(region: &str, account: &str, uuid: &str) -> String {
    format!("arn:aws:lambda:{region}:{account}:event-source-mapping:{uuid}")
}

/// A layer version ARN taken apart: (region, account, name, version).
pub(crate) fn split_layer(arn: &str) -> Option<(&str, &str, &str, u32)> {
    match arn.split(':').collect::<Vec<_>>().as_slice() {
        ["arn", _, "lambda", r, a, "layer", name, v] => {
            let version = v.parse().ok()?;
            Some((r, a, name, version))
        }
        _ => None,
    }
}

/// The region and account of an ARN, whatever service it is for.
pub(crate) fn region_account(arn: &str) -> Option<(&str, &str)> {
    let mut it = arn.split(':');
    let (_, _, _, region, account) = (it.next()?, it.next()?, it.next()?, it.next()?, it.next()?);
    Some((region, account))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_form_names_the_same_function() {
        let p = |t: &str, q: Option<&str>| parse(t, q, "us-east-1", "111122223333").unwrap();
        let plain = FnRef {
            name: "f".into(),
            qualifier: None,
        };
        assert_eq!(p("f", None), plain);
        assert_eq!(
            p("arn:aws:lambda:us-east-1:111122223333:function:f", None),
            plain
        );
        assert_eq!(p("111122223333:function:f", None), plain);
        let live = FnRef {
            name: "f".into(),
            qualifier: Some("live".into()),
        };
        assert_eq!(p("f:live", None), live);
        assert_eq!(p("f", Some("live")), live);
        assert_eq!(
            p(
                "arn:aws:lambda:us-east-1:111122223333:function:f:live",
                Some("live")
            ),
            live
        );
        assert!(parse("f:live", Some("other"), "us-east-1", "111122223333").is_err());
        assert!(
            parse(
                "arn:aws:lambda:eu-west-1:111122223333:function:f",
                None,
                "us-east-1",
                "111122223333"
            )
            .is_err()
        );
    }
}
