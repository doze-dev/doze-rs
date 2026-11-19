//! Which services a policy grants: the namespaces its `Allow` statements' actions name. The last-accessed reports and
//! `ListPoliciesGrantingServiceAccess` list services this way; AWS says they apply "only permissions policy logic" (the model), so
//! boundaries, resource policies and denies do not narrow them.
//!
//! An action with a wildcard in its service part (`*`, `s3*:...`), or a `NotAction`, grants every service it can match. AWS matches
//! those against all of its services (several hundred); here they are matched against the services doze-aws runs, listed below.

use std::collections::BTreeSet;

use aws_iam_policy::{Document, Effect};

/// The services doze-aws runs: namespace and the name AWS gives it in a last-accessed report (memory: the names).
pub(crate) const SERVICES: [(&str, &str); 19] = [
    ("apigateway", "Amazon API Gateway"),
    ("cloudformation", "AWS CloudFormation"),
    ("cloudwatch", "Amazon CloudWatch"),
    ("dynamodb", "Amazon DynamoDB"),
    ("events", "Amazon EventBridge"),
    ("execute-api", "Amazon API Gateway"),
    ("iam", "AWS Identity and Access Management"),
    ("kinesis", "Amazon Kinesis"),
    ("kms", "AWS Key Management Service"),
    ("lambda", "AWS Lambda"),
    ("logs", "Amazon CloudWatch Logs"),
    ("s3", "Amazon S3"),
    ("scheduler", "Amazon EventBridge Scheduler"),
    ("secretsmanager", "AWS Secrets Manager"),
    ("sns", "Amazon SNS"),
    ("sqs", "Amazon SQS"),
    ("ssm", "AWS Systems Manager"),
    ("states", "AWS Step Functions"),
    ("sts", "AWS Security Token Service"),
];

/// The name of a service, or its namespace when doze-aws does not run it.
pub(crate) fn service_name(namespace: &str) -> &str {
    SERVICES
        .iter()
        .find(|(ns, _)| *ns == namespace)
        .map_or(namespace, |(_, name)| name)
}

/// The namespaces `doc` grants.
pub(crate) fn namespaces(doc: &Document, out: &mut BTreeSet<String>) {
    for s in doc.statements.iter().filter(|s| s.effect == Effect::Allow) {
        let Some(actions) = &s.actions else { continue };
        let services: Vec<String> = actions
            .patterns
            .iter()
            .map(|p| {
                p.split_once(':')
                    .map_or(p.as_str(), |(ns, _)| ns)
                    .to_ascii_lowercase()
            })
            .collect();
        if actions.not {
            // Every service but one whose every action is excluded (`ns:*`, or `*`).
            let whole: Vec<&String> = actions
                .patterns
                .iter()
                .filter(|p| p.as_str() == "*" || p.ends_with(":*"))
                .collect();
            for (ns, _) in SERVICES {
                if !whole
                    .iter()
                    .any(|p| glob(&p.to_ascii_lowercase(), &format!("{ns}:*")))
                {
                    out.insert(ns.to_owned());
                }
            }
            continue;
        }
        for ns in services {
            if ns.contains(['*', '?']) {
                out.extend(
                    SERVICES
                        .iter()
                        .filter(|(known, _)| glob(&ns, known))
                        .map(|(known, _)| (*known).to_owned()),
                );
            } else if !ns.is_empty() {
                out.insert(ns);
            }
        }
    }
}

/// Whether `doc` grants `namespace`.
pub(crate) fn grants(doc: &Document, namespace: &str) -> bool {
    let mut found = BTreeSet::new();
    namespaces(doc, &mut found);
    found.contains(namespace)
}

/// IAM's wildcard match: `*` any run, `?` one character.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some(pi);
                mark = ti;
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some(s) => {
                    pi = s + 1;
                    mark += 1;
                    ti = mark;
                }
                None => return false,
            },
        }
    }
    p.get(pi..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn of(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        namespaces(&aws_iam_policy::parse(text).unwrap().unwrap(), &mut out);
        out
    }

    #[test]
    fn a_policy_grants_the_services_its_allows_name() {
        let doc = r#"{"Version":"2012-10-17","Statement":[
            {"Effect":"Allow","Action":["s3:GetObject","SQS:SendMessage","ec2:Describe*"],"Resource":"*"},
            {"Effect":"Deny","Action":"kms:*","Resource":"*"}]}"#;
        assert_eq!(of(doc), ["ec2", "s3", "sqs"].map(String::from).into());
        let all = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"*"}]}"#;
        assert_eq!(of(all).len(), SERVICES.len());
        let s = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"s*:List*","Resource":"*"}]}"#;
        assert_eq!(
            of(s),
            [
                "s3",
                "scheduler",
                "secretsmanager",
                "sns",
                "sqs",
                "ssm",
                "states",
                "sts"
            ]
            .map(String::from)
            .into()
        );
        let not = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","NotAction":["iam:*","sts:*","s3:GetObject"],"Resource":"*"}]}"#;
        let got = of(not);
        assert!(got.contains("s3") && !got.contains("iam") && !got.contains("sts"));
        assert_eq!(service_name("sqs"), "Amazon SQS");
        assert_eq!(service_name("ec2"), "ec2");
    }
}
