//! The services this build serves, by the names a person types (`--services sqs,s3`), and the stand-in a service left out of
//! `--services` is replaced by: it answers every call from outside (the gateway) with how to turn it on; the other services still reach the service itself.

use std::sync::Arc;

use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service, Web};

/// One service, as the command line and `doze-aws.toml` name it.
#[derive(Debug)]
pub(crate) struct Svc {
    /// The name in `--services`.
    pub(crate) name: &'static str,
    /// The name a person reads.
    pub(crate) title: &'static str,
    /// Other names accepted for it (its signing or endpoint names).
    pub(crate) aliases: &'static [&'static str],
}

const fn svc(name: &'static str, title: &'static str, aliases: &'static [&'static str]) -> Svc {
    Svc {
        name,
        title,
        aliases,
    }
}

/// Every service, in the order they are listed.
pub(crate) const ALL: &[Svc] = &[
    svc("s3", "S3", &[]),
    svc("sqs", "SQS", &[]),
    svc("sns", "SNS", &[]),
    svc("dynamodb", "DynamoDB", &["dynamodbstreams", "streams"]),
    svc("lambda", "Lambda", &[]),
    svc(
        "apigateway",
        "API Gateway",
        &["apigatewayv2", "execute-api"],
    ),
    svc("stepfunctions", "Step Functions", &["states", "sfn"]),
    svc("eventbridge", "EventBridge", &["events"]),
    svc(
        "scheduler",
        "EventBridge Scheduler",
        &["eventbridgescheduler"],
    ),
    svc("kinesis", "Kinesis", &[]),
    svc("logs", "CloudWatch Logs", &["cloudwatchlogs"]),
    svc("cloudwatch", "CloudWatch", &["monitoring", "metrics"]),
    svc("secretsmanager", "Secrets Manager", &["secrets"]),
    svc("ssm", "SSM Parameter Store", &["parameterstore"]),
    svc("kms", "KMS", &[]),
    svc("iam", "IAM", &[]),
    svc("sts", "STS", &[]),
    svc("cloudformation", "CloudFormation", &["cfn"]),
];

/// The service `name` names, by its name or an alias, in any case.
pub(crate) fn find(name: &str) -> Option<&'static Svc> {
    let name = name.trim().to_ascii_lowercase();
    ALL.iter()
        .find(|s| s.name == name || s.aliases.contains(&name.as_str()))
}

/// The services a `--services` list names (`all` is every one), in catalogue order, or why the list is wrong.
pub(crate) fn parse_list(items: &[String]) -> Result<Vec<&'static Svc>, String> {
    if items.is_empty() || items.iter().any(|i| i.trim().eq_ignore_ascii_case("all")) {
        return Ok(ALL.iter().collect());
    }
    let mut chosen = Vec::new();
    for item in items.iter().filter(|i| !i.trim().is_empty()) {
        let Some(found) = find(item) else {
            return Err(unknown(item));
        };
        if !chosen.iter().any(|c: &&Svc| c.name == found.name) {
            chosen.push(found);
        }
    }
    chosen.sort_by_key(|s| ALL.iter().position(|a| a.name == s.name));
    Ok(chosen)
}

fn unknown(item: &str) -> String {
    let close = ALL
        .iter()
        .flat_map(|s| std::iter::once(s.name).chain(s.aliases.iter().copied()))
        .map(|n| (strsim::jaro_winkler(n, &item.to_ascii_lowercase()), n))
        .filter(|(score, _)| *score > 0.8)
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let names: Vec<&str> = ALL.iter().map(|s| s.name).collect();
    let hint = close.map_or_else(String::new, |(_, n)| format!(" (did you mean `{n}`?)"));
    format!(
        "`{item}` is not a service doze-aws serves{hint}; the services are: {}",
        names.join(", ")
    )
}

/// A service left out of `--services`: its model, so a call is understood and answered in its own protocol, and a refusal for
/// every call.
pub(crate) struct Off {
    inner: Arc<dyn Service>,
    refusal: ApiError,
}

impl std::fmt::Debug for Off {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Off").finish_non_exhaustive()
    }
}

impl Off {
    /// `inner` stood in for: the refusal names it and says how to turn it on, given the services that are on.
    pub(crate) fn new(inner: Arc<dyn Service>, svc: &Svc, on: &[&Svc]) -> Self {
        let on: Vec<&str> = on.iter().map(|s| s.name).collect();
        // (memory) OptInRequired, 403: what AWS says to an account that has not opted into a service. Not recorded; the nearest
        // AWS answer to "this endpoint exists but does not serve you".
        let refusal = ApiError::new(
            403,
            "OptInRequired",
            format!(
                "doze-aws is not serving {title}: it was started with --services {list}. To serve it, add {name} to --services \
                 (or to `services` in doze-aws.toml) and start doze-aws again.",
                title = svc.title,
                list = on.join(","),
                name = svc.name,
            ),
        );
        Self { inner, refusal }
    }
}

impl Service for Off {
    fn meta(&self) -> &'static ServiceMeta {
        self.inner.meta()
    }

    fn tables(&self) -> &'static Tables {
        self.inner.tables()
    }

    fn flat_client_errors(&self) -> bool {
        self.inner.flat_client_errors()
    }

    fn call(&self, _call: Call) -> BoxFuture<'_, Reply> {
        let refusal = self.refusal.clone();
        Box::pin(async move { Err(refusal) })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        self.inner
            .rest()
            .is_some()
            .then_some(self as &dyn RestService)
    }

    fn web(&self) -> Option<&dyn Web> {
        None
    }

    fn invalid(&self, _violation: &aws_schema::json::Violation) -> ApiError {
        self.refusal.clone()
    }
}

impl RestService for Off {
    fn call_rest(
        &self,
        _call: Call,
        _upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        let refusal = self.refusal.clone();
        Box::pin(async move { Err(refusal) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Result<Vec<&'static str>, String> {
        let items: Vec<String> = list.iter().map(|s| (*s).to_owned()).collect();
        parse_list(&items).map(|v| v.iter().map(|s| s.name).collect())
    }

    #[test]
    fn a_list_is_read_by_name_or_alias_deduplicated_in_catalogue_order() {
        assert_eq!(
            names(&["states", "SQS", "s3", "sqs"]).unwrap(),
            vec!["s3", "sqs", "stepfunctions"]
        );
        assert_eq!(names(&[]).unwrap().len(), ALL.len());
        assert_eq!(names(&["all"]).unwrap().len(), ALL.len());
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_nearest_one() {
        let e = names(&["dynamdb"]).unwrap_err();
        assert!(e.contains("did you mean `dynamodb`"), "{e}");
        assert!(e.contains("cloudformation"), "{e}");
    }
}
