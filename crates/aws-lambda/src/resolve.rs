//! What a runner needs to invoke a function, found by the actor that owns the state: the version an alias or number names, its
//! configuration and code, its layers, and whether the caller may invoke it.

use std::path::PathBuf;

use aws_iam_policy::{Context, Policies, Request, Subject};
use aws_peers::{Mode, Need};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Map, Value};

use crate::arn;
use crate::core::Core;
use crate::errors::no_function;
use crate::functions::LATEST;
use crate::model::Code;
use crate::req::Req;

/// A request to invoke.
#[derive(Debug)]
pub(crate) struct Resolving {
    pub(crate) caller: Caller,
    /// `FunctionName` as the caller gave it, and the separate `Qualifier`.
    pub(crate) function: String,
    pub(crate) qualifier: Option<String>,
    /// For a service acting for someone (S3, SNS, EventBridge): the ARN of the thing that invokes, for the policy's conditions.
    pub(crate) source_arn: Option<String>,
    /// A number in `[0, 1)` that picks between the versions of a weighted alias.
    pub(crate) draw: f64,
    /// Whether the caller is a function URL (its permission is `lambda:InvokeFunctionUrl`).
    pub(crate) url: bool,
    /// Lambda itself invokes (a destination): no permission is asked.
    pub(crate) internal: bool,
}

/// What a runner is told.
#[derive(Debug)]
pub(crate) struct Resolved {
    pub(crate) name: String,
    pub(crate) region: String,
    /// The ARN the caller invoked (qualified when the call was).
    pub(crate) invoked_arn: String,
    /// The version that runs: `$LATEST` or a number.
    pub(crate) version: String,
    pub(crate) config: Map<String, Value>,
    pub(crate) code: Code,
    pub(crate) root: PathBuf,
    /// Each layer in the order listed: its directory, or its ARN when it is not kept here.
    pub(crate) layers: Vec<Result<PathBuf, String>>,
    pub(crate) revision: String,
    pub(crate) reserved: Option<i64>,
    /// The asynchronous invocation settings for the version.
    pub(crate) event_invoke: Option<Value>,
}

impl Core {
    pub(crate) async fn resolve(&self, ask: &Resolving) -> Result<Box<Resolved>, ApiError> {
        let f = arn::parse(
            &ask.function,
            ask.qualifier.as_deref(),
            &self.region,
            &ask.caller.account_id,
        )?;
        let shown = Some(f.qualifier.as_deref().unwrap_or(LATEST));
        let invoked = arn::function(&self.region, &ask.caller.account_id, &f.name, shown);
        let function = self
            .functions
            .get(&f.name)
            .ok_or_else(|| no_function(&invoked))?;
        let key = f.qualifier.clone().unwrap_or_default();
        // What the caller named is what the policy is asked about: the unqualified ARN unless they gave a qualifier.
        let named = arn::function(
            &self.region,
            &ask.caller.account_id,
            &f.name,
            f.qualifier.as_deref(),
        );
        self.allow(ask, &named, function.policies.get(&key).map(String::as_str))
            .await?;
        let qualifier = match f.qualifier.as_deref() {
            Some(q) if function.aliases.iter().any(|a| a.name == q) => Some(
                self.alias_version(function, q, ask.draw)
                    .unwrap_or_else(|| q.to_owned()),
            ),
            other => other.map(str::to_owned),
        };
        let probe = Req {
            body: Value::Null,
            caller: ask.caller.clone(),
        };
        let (snap, version) = self
            .snap_of(&probe, function, qualifier.as_deref())
            .map_err(|_| no_function(&invoked))?;
        let config: Map<String, Value> = serde_json::from_str(&snap.config).unwrap_or_default();
        let layer_arns: Vec<String> = config
            .get("Layers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let event_key = if version == LATEST {
            LATEST.to_owned()
        } else {
            f.qualifier.clone().unwrap_or_else(|| version.clone())
        };
        Ok(Box::new(Resolved {
            name: f.name.clone(),
            region: self.region.clone(),
            invoked_arn: invoked,
            root: self.code_root(&snap.code),
            code: snap.code.clone(),
            layers: self.layer_dirs(&layer_arns),
            revision: snap.revision.clone(),
            reserved: function.reserved,
            event_invoke: function
                .event_invoke
                .get(&event_key)
                .or_else(|| function.event_invoke.get(LATEST))
                .and_then(|s| serde_json::from_str(s).ok()),
            config,
            version,
        }))
    }

    /// Whether the caller may invoke: a person or role by their identity policies and the function's own policy; a service by the
    /// function's policy alone (a statement naming its principal, with `aws:SourceArn` when it gave one).
    async fn allow(
        &self,
        ask: &Resolving,
        arn: &str,
        policy: Option<&str>,
    ) -> Result<(), ApiError> {
        let Some(peers) = self
            .peers
            .clone()
            .filter(|p| p.mode() != Mode::Off && !ask.internal)
        else {
            return Ok(());
        };
        let anonymous_url = ask.url && ask.caller.access_key_id == Caller::ANONYMOUS;
        let refusal = match ask.caller.service.as_deref() {
            Some(service) => Self::service_may(service, ask, arn, policy),
            None if anonymous_url => anyone_may(arn, policy),
            None => {
                let action = if ask.url {
                    "lambda:InvokeFunctionUrl"
                } else {
                    "lambda:InvokeFunction"
                };
                let need = Need::new(action, arn).with_policy(policy.map(str::to_owned));
                peers
                    .authorize(&ask.caller, vec![need])
                    .await
                    .map_err(|e| e.message)
            }
        };
        match refusal {
            Ok(()) => Ok(()),
            Err(why) if peers.mode() == Mode::Soft => {
                if anonymous_url {
                    tracing::warn!(
                        "iam soft mode: AWS answers this function URL request 403: {why}"
                    );
                } else {
                    tracing::warn!("iam soft mode: this invocation would be refused: {why}");
                }
                Ok(())
            }
            Err(why) => Err(ApiError::new(403, "AccessDeniedException", why)),
        }
    }

    fn service_may(
        service: &str,
        ask: &Resolving,
        arn: &str,
        policy: Option<&str>,
    ) -> Result<(), String> {
        let principal = if service.contains('.') {
            service.to_owned()
        } else {
            format!("{service}.amazonaws.com")
        };
        let refused = || {
            format!(
                "{principal} is not authorized to perform: lambda:InvokeFunction on resource: {arn} because no resource-based policy allows the lambda:InvokeFunction action"
            )
        };
        let Some(Ok(doc)) = policy.and_then(aws_iam_policy::parse) else {
            return Err(refused());
        };
        let mut context = Context::new();
        context
            .set("aws:SourceAccount", ask.caller.account_id.clone())
            .set("aws:PrincipalServiceName", principal.clone());
        if let Some(source) = &ask.source_arn {
            context.set("aws:SourceArn", source.clone());
        }
        let subject = Subject {
            arns: Vec::new(),
            account: ask.caller.account_id.clone(),
            service: Some(principal.clone()),
        };
        let resource = [&doc];
        let policies = Policies {
            resource: Some(&resource),
            ..Policies::default()
        };
        let request = Request {
            action: "lambda:InvokeFunction",
            resource: arn,
            context: &context,
            subject: &subject,
        };
        if aws_iam_policy::evaluate(&policies, &request).allowed() {
            Ok(())
        } else {
            Err(refused())
        }
    }
}

/// What AWS answers a function URL call that has `lambda:InvokeFunctionUrl` but not `lambda:InvokeFunction` (recorded 2026-11-15).
const URL_NEEDS_BOTH: &str = "Forbidden. Invoking a Function URL requires both the lambda:InvokeFunctionUrl and lambda:InvokeFunction permissions. For troubleshooting, see: https://docs.aws.amazon.com/lambda/latest/dg/urls-auth.html";

/// Whether anyone may call a function URL whose `AuthType` is `NONE`. AWS asks the function's resource policy for two grants to
/// `Principal` `*`: `lambda:InvokeFunctionUrl` (with `lambda:FunctionUrlAuthType` `NONE`) and, since October 2025,
/// `lambda:InvokeFunction` (with `lambda:InvokedViaFunctionUrl` `true`). Recorded 2026-11-15: with the first alone AWS answers
/// 403 and `{"Message": "Forbidden. Invoking a Function URL requires both ..."}`; with both, the function runs. The refusal is that
/// message, then doze's hint; with neither grant AWS's words are `Forbidden` alone `(memory)`.
pub(crate) fn anyone_may(arn: &str, policy: Option<&str>) -> Result<(), String> {
    let doc = policy.and_then(aws_iam_policy::parse).and_then(Result::ok);
    let mut context = Context::new();
    context
        .set("lambda:FunctionUrlAuthType", "NONE")
        .set("lambda:InvokedViaFunctionUrl", "true");
    let subject = Subject {
        arns: Vec::new(),
        account: String::new(),
        service: None,
    };
    let granted = |action: &str| {
        doc.as_ref().is_some_and(|d| {
            let resource = [d];
            let policies = Policies {
                resource: Some(&resource),
                ..Policies::default()
            };
            let request = Request {
                action,
                resource: arn,
                context: &context,
                subject: &subject,
            };
            aws_iam_policy::evaluate(&policies, &request).allowed()
        })
    };
    let missing: Vec<&str> = ["lambda:InvokeFunctionUrl", "lambda:InvokeFunction"]
        .into_iter()
        .filter(|a| !granted(a))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let aws = if missing == ["lambda:InvokeFunction"] {
        URL_NEEDS_BOTH
    } else {
        "Forbidden"
    };
    Err(format!(
        "{aws} doze-aws: a function URL with AuthType NONE needs the function's resource policy to allow {} to Principal \"*\". Add them with \
         `aws lambda add-permission --principal '*' --action lambda:InvokeFunctionUrl --function-url-auth-type NONE` and \
         `aws lambda add-permission --principal '*' --action lambda:InvokeFunction --invoked-via-function-url` \
         (each with --function-name and its own --statement-id).",
        missing.join(" and ")
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{URL_NEEDS_BOTH, anyone_may};

    const ARN: &str = "arn:aws:lambda:us-east-1:111122223333:function:f";

    fn grant(sid: &str, action: &str, condition: &serde_json::Value) -> serde_json::Value {
        json!({"Sid": sid, "Effect": "Allow", "Principal": "*", "Action": action, "Resource": ARN, "Condition": condition})
    }

    #[test]
    fn a_none_url_needs_both_grants_to_anyone() {
        let url = grant(
            "url",
            "lambda:InvokeFunctionUrl",
            &json!({"StringEquals": {"lambda:FunctionUrlAuthType": "NONE"}}),
        );
        let invoke = grant(
            "invoke",
            "lambda:InvokeFunction",
            &json!({"Bool": {"lambda:InvokedViaFunctionUrl": "true"}}),
        );
        let doc = |s: Vec<serde_json::Value>| {
            json!({"Version": "2012-10-17", "Statement": s}).to_string()
        };
        assert!(anyone_may(ARN, Some(&doc(vec![url.clone(), invoke]))).is_ok());
        let only_url = anyone_may(ARN, Some(&doc(vec![url]))).unwrap_err();
        assert!(
            only_url.starts_with(&format!("{URL_NEEDS_BOTH} doze-aws: ")),
            "{only_url}"
        );
        assert!(
            anyone_may(ARN, None)
                .unwrap_err()
                .starts_with("Forbidden doze-aws: ")
        );
    }
}
