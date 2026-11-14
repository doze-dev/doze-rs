//! The anonymous principal: a request that carried no signature at all (S3 with `curl` or a browser, a website page). AWS lets it do
//! only what a resource policy grants to `"Principal": "*"` (S3 adds what a public ACL grants, as statements of the same kind); no
//! identity policy can speak for it. Answered without the actor: nothing about it is stored.
//!
//! Which requests get here is the gateway's call, and depends on the flags (written down in `docs/port/aws/stages/correctness.md`):
//! `--iam off` asks nothing, so an unsigned request is served as before; `--iam soft` (the default) logs what would be refused,
//! with what to do about it, and serves it; `--iam enforce` refuses it.

use aws_iam_policy::{Context, Decision, Policies, Request, Subject, Verdict};
use aws_peers::{Mode, Need};
use aws_wire::ApiError;

use crate::accesslog::Seen;

/// How the access log and the refusals name the caller.
pub(crate) const SHOWN: &str = "anonymous";

/// Whether each of `needs` is granted to anyone by its resource policy. In soft mode a refusal is logged and the answer is yes.
pub(crate) fn authorize(
    mode: Mode,
    needs: &[Need],
    now_ms: i64,
) -> (Result<(), ApiError>, Vec<Seen>) {
    let mut seen = Vec::with_capacity(needs.len());
    for need in needs {
        let decision = decide(need, now_ms);
        let allowed = decision.verdict == Verdict::Allow;
        seen.push(Seen {
            principal: SHOWN.to_owned(),
            role: None,
            action: need.action.clone(),
            resource: need.resource.clone(),
            decision: match decision.verdict {
                Verdict::Allow => "allowed",
                Verdict::ExplicitDeny => "explicitDeny",
                Verdict::ImplicitDeny => "implicitDeny",
            },
            reason: if allowed {
                "allowed by the resource-based policy, which grants it to anyone".to_owned()
            } else {
                hint(need)
            },
            at_ms: now_ms,
        });
        if allowed {
            continue;
        }
        if mode == Mode::Enforce {
            // S3's words for an anonymous refusal; it is the only service an unsigned request reaches.
            return (
                Err(ApiError::new(403, "AccessDeniedException", "Access Denied")),
                seen,
            );
        }
        tracing::warn!(
            action = %need.action,
            resource = %need.resource,
            "iam soft mode: would be refused: anonymous, {} on {} — {}",
            need.action,
            need.resource,
            hint(need)
        );
    }
    (Ok(()), seen)
}

/// What a developer can do about a refusal: grant it to anyone in the resource's policy, or sign the request.
fn hint(need: &Need) -> String {
    if need.action.starts_with("s3:") {
        "add a bucket policy granting Principal * (with the bucket's public access block off) or sign the request".to_owned()
    } else {
        "add a resource policy granting Principal * or sign the request".to_owned()
    }
}

fn decide(need: &Need, now_ms: i64) -> Decision {
    let resource: Option<Vec<aws_iam_policy::Document>> = need
        .policy
        .as_deref()
        .and_then(aws_iam_policy::parse)
        .and_then(Result::ok)
        .map(|d| vec![d]);
    let refs: Option<Vec<&aws_iam_policy::Document>> =
        resource.as_ref().map(|v| v.iter().collect());
    let policies = Policies {
        identity: &[],
        boundary: None,
        session: None,
        resource: refs.as_deref(),
        resource_gate: false,
    };
    let mut context = Context::new();
    let now = now_ms / 1000;
    context
        .set("aws:CurrentTime", aws_core::time::format_iso8601(now))
        .set("aws:EpochTime", now.to_string())
        .set("aws:SecureTransport", "true")
        .set("aws:ViaAWSService", "false");
    for key in [
        "aws:SourceIp",
        "aws:SourceVpc",
        "aws:SourceVpce",
        "aws:UserAgent",
        "aws:Referer",
    ] {
        context.leave_unsupplied(key);
    }
    for (key, values) in &need.context {
        context.set_many(key, values.clone());
    }
    // Nobody: no ARN, no account. Only a grant to `*` (or `{"AWS": "*"}`) names it.
    let subject = Subject::default();
    let request = Request {
        action: &need.action,
        resource: &need.resource,
        context: &context,
        subject: &subject,
    };
    aws_iam_policy::evaluate(&policies, &request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn need(policy: Option<&str>) -> Need {
        Need::new("s3:GetObject", "arn:aws:s3:::b/k").with_policy(policy.map(str::to_owned))
    }

    const PUBLIC: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::b/*"}]}"#;
    const ACCOUNT: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"AWS":"arn:aws:iam::111122223333:root"},"Action":"s3:GetObject","Resource":"arn:aws:s3:::b/*"}]}"#;

    #[test]
    fn only_a_grant_to_anyone_lets_anonymous_in() {
        assert!(authorize(Mode::Enforce, &[need(Some(PUBLIC))], 0).0.is_ok());
        for refused in [None, Some(ACCOUNT)] {
            let (verdict, seen) = authorize(Mode::Enforce, &[need(refused)], 0);
            let e = verdict.err().map(|e| (e.status, e.message.to_string()));
            assert_eq!(e, Some((403, "Access Denied".to_owned())));
            assert_eq!(seen.first().map(|s| s.decision), Some("implicitDeny"));
            assert!(
                seen.first()
                    .is_some_and(|s| s.reason.contains("bucket policy granting Principal *"))
            );
        }
    }

    #[test]
    fn soft_mode_logs_and_lets_it_through() {
        let (verdict, seen) = authorize(Mode::Soft, &[need(None)], 0);
        assert!(verdict.is_ok());
        assert_eq!(seen.first().map(|s| s.principal.as_str()), Some(SHOWN));
    }
}
