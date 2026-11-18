//! The namespace check AWS makes on a name under `/aws/` before it looks anything up.
//!
//! Recorded (conformance `test_ssm.py::test_public_parameters_are_read_only`): a name whose third segment is no namespace AWS
//! publishes under is refused with `AccessDeniedException`, `No access to "/aws/" namespace: aws/service/<segment> is not a valid
//! namespace`, by `GetParameter`, by `GetParametersByPath`, by `GetParameters` (the whole call, not one entry of
//! `InvalidParameters`) and by `DescribeParameters` given the prefix in a `Name` `BeginsWith` filter (`/aws/service/` itself
//! names `aws/service/`). A name AWS does not publish inside a real namespace is an ordinary miss (`ParameterNotFound`, or an
//! entry of `InvalidParameters`).

use aws_wire::ApiError;

/// The namespaces AWS publishes public parameters under (`/aws/service/<namespace>/...`). The ones doze-aws serves names from
/// (see `public.rs`) and, beside them, the other ones in common use (memory), so a real name doze-aws does not serve is a miss and
/// not a refusal.
const NAMESPACES: &[&str] = &[
    "ami-amazon-linux-latest",
    "ami-windows-latest",
    "aws-for-fluent-bit",
    "bottlerocket",
    "canonical",
    "cloud9",
    "datasync",
    "debian",
    "deeplearning",
    "ecs",
    "eks",
    "freebsd",
    "global-infrastructure",
    "marketplace",
    "neuron",
    "powershell",
    "storagegateway",
    "suse",
];

/// The namespace `name` (a parameter name, a path or a name prefix under `/aws/`) falls in: its first three segments, without
/// the leading slash.
fn of(name: &str) -> String {
    let path = name.strip_prefix('/').unwrap_or(name);
    path.splitn(4, '/').take(3).collect::<Vec<_>>().join("/")
}

/// Refuse `name` if it is under `/aws/` and outside every namespace AWS publishes in, as AWS does.
pub(crate) fn check(name: &str) -> Result<(), ApiError> {
    if !name.starts_with("/aws/") {
        return Ok(());
    }
    let namespace = of(name);
    let known = namespace
        .strip_prefix("aws/service/")
        .is_some_and(|s| NAMESPACES.contains(&s));
    if known {
        return Ok(());
    }
    Err(ApiError::modeled(
        "AccessDeniedException",
        format!("No access to \"/aws/\" namespace: {namespace} is not a valid namespace"),
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_name_is_held_to_its_first_three_segments() {
        assert_eq!(
            of("/aws/service/not-a-parameter"),
            "aws/service/not-a-parameter"
        );
        assert_eq!(
            of("/aws/service/not-a-namespace/image"),
            "aws/service/not-a-namespace"
        );
        assert_eq!(of("/aws/service/"), "aws/service/");
    }

    #[test]
    fn only_names_outside_every_namespace_are_refused() {
        assert!(check("/aws/service/ami-amazon-linux-latest/nothing").is_ok());
        assert!(check("/aws/service/debian/release/12/latest/amd64").is_ok());
        assert!(check("/app/x").is_ok());
        let e = check("/aws/service/").unwrap_err();
        assert_eq!(
            e.message,
            "No access to \"/aws/\" namespace: aws/service/ is not a valid namespace"
        );
        assert!(check("/aws/service/not-a-parameter").is_err());
    }
}
