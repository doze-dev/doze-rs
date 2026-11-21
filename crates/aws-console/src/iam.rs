//! What IAM can tell the console, asked in this process on IAM's own doze-only addresses (`/_doze/iam/...`, the ones
//! `doze-aws` documents for the access log): who an access key is (`DozePrincipals`) and every authorization question it
//! answered (`DozeAccessLog`). Neither is an AWS call, so neither shows in Live, and neither is asked until a page asks.

use std::collections::{BTreeSet, HashMap};

use aws_wire::service::{WebRequest, WebSignature};
use serde_json::Value;

use crate::api::at;
use crate::facts::Facts;
use crate::model::{KeyPrincipal, PrincipalKind, Refusal};
use crate::resources::bus::CONSOLE;

/// Asks IAM's doze-only address `action` with `query`; its JSON answer, or `None` when IAM is not in the build or did not
/// answer with JSON.
async fn ask(facts: &Facts, action: &str, query: String) -> Option<Value> {
    let web = facts.api("IAM")?.web()?;
    let reply = web
        .serve(WebRequest {
            method: "GET".to_owned(),
            host: "localhost".to_owned(),
            path: format!("/_doze/iam/{action}"),
            query,
            headers: Vec::new(),
            body: axum::body::Bytes::new(),
            endpoint: facts.endpoint.clone(),
            signature: WebSignature::Unsigned,
        })
        .await;
    (reply.status == 200)
        .then(|| serde_json::from_slice(&reply.body).ok())
        .flatten()
}

/// The kind of caller `key` is, from its shape alone (what IAM itself goes by: `AKIA` it may have issued, `ASIA` is a
/// session, anything else is the account root's).
fn shape(key: &str) -> PrincipalKind {
    if key == CONSOLE {
        PrincipalKind::Console
    } else if key.starts_with("ASIA") {
        PrincipalKind::Session
    } else {
        PrincipalKind::Root
    }
}

/// Each of `keys`, named: a key IAM issued by its user, any other by its kind. IAM is asked once, and only about keys it may
/// have issued (none wakes it when there are none).
pub async fn principals<'a>(
    facts: &Facts,
    keys: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, KeyPrincipal> {
    let keys: BTreeSet<&str> = keys.into_iter().filter(|k| !k.is_empty()).collect();
    let issued: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|k| k.starts_with("AKIA"))
        .collect();
    let mut users: HashMap<String, (String, String)> = HashMap::new();
    if !issued.is_empty()
        && let Some(answer) = ask(
            facts,
            "DozePrincipals",
            format!("AccessKeyIds={}", issued.join(",")),
        )
        .await
    {
        for p in at(&answer, "Principals").as_array().into_iter().flatten() {
            if let (Some(k), Some(n), Some(a)) = (
                at(p, "AccessKeyId").as_str(),
                at(p, "UserName").as_str(),
                at(p, "Arn").as_str(),
            ) {
                users.insert(k.to_owned(), (n.to_owned(), a.to_owned()));
            }
        }
    }
    let root = format!("arn:aws:iam::{}:root", facts.account);
    keys.into_iter()
        .map(|key| {
            let named = match users.remove(key) {
                Some((name, arn)) => KeyPrincipal {
                    key: key.to_owned(),
                    kind: PrincipalKind::User,
                    name: Some(name),
                    arn: Some(arn),
                },
                None => {
                    let kind = shape(key);
                    KeyPrincipal {
                        key: key.to_owned(),
                        kind,
                        name: None,
                        arn: (kind == PrincipalKind::Root).then(|| root.clone()),
                    }
                }
            };
            (key.to_owned(), named)
        })
        .collect()
}

/// The named principal of `key` from `named`, or one by its shape when it was not asked about.
#[must_use]
pub fn principal_of(named: &HashMap<String, KeyPrincipal>, key: &str) -> KeyPrincipal {
    named.get(key).cloned().unwrap_or_else(|| KeyPrincipal {
        key: key.to_owned(),
        kind: shape(key),
        name: None,
        arn: None,
    })
}

/// IAM's access log as it stands: its refusals, newest first, and how many rows it let go. `None` when it could not be read.
pub async fn refusals(facts: &Facts) -> Option<(Vec<Refusal>, u64)> {
    let log = ask(facts, "DozeAccessLog", String::new()).await?;
    let mut out: Vec<Refusal> = at(&log, "Entries")
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| at(e, "Decision").as_str().is_some_and(|d| d != "allowed"))
        .map(|e| {
            let text = |k: &str| e[k].as_str().unwrap_or_default().to_owned();
            Refusal {
                principal: text("Principal"),
                action: text("Action"),
                resource: text("Resource"),
                decision: text("Decision"),
                reason: text("Reason"),
                count: at(e, "Count").as_u64().unwrap_or(0),
                last_seen: text("LastSeen"),
            }
        })
        .collect();
    // ISO 8601 times sort as text.
    out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    Some((out, at(&log, "Evicted").as_u64().unwrap_or(0)))
}

/// The refusals that explain calls of `principal` to `operation`, and whether they were matched by service alone. The same
/// principal (a session: any assumed role, which is all IAM can say without its token), and an IAM action named as the
/// operation is (`sqs:SendMessage` for `SendMessage`); when none is, the principal's refusals of the service's actions
/// (`prefix`, its signing name: S3's `ListObjectsV2` is checked as `s3:ListBucket`).
#[must_use]
pub fn explaining(
    refusals: &[Refusal],
    principal: &KeyPrincipal,
    operation: &str,
    prefix: &str,
) -> (Vec<Refusal>, bool) {
    let mine: Vec<&Refusal> = refusals
        .iter()
        .filter(|r| match principal.kind {
            PrincipalKind::User => principal.arn.as_deref() == Some(r.principal.as_str()),
            PrincipalKind::Session => r.principal.contains(":assumed-role/"),
            PrincipalKind::Root | PrincipalKind::Console => false,
        })
        .collect();
    let named: Vec<Refusal> = mine
        .iter()
        .filter(|r| {
            r.action
                .split_once(':')
                .is_some_and(|(_, op)| op == operation)
        })
        .map(|r| (*r).clone())
        .collect();
    if !named.is_empty() || prefix.is_empty() {
        return (named, false);
    }
    let by_service: Vec<Refusal> = mine
        .iter()
        .filter(|r| r.action.split_once(':').is_some_and(|(p, _)| p == prefix))
        .map(|r| (*r).clone())
        .collect();
    let found = !by_service.is_empty();
    (by_service, found)
}

#[cfg(test)]
mod tests {
    use super::{explaining, shape};
    use crate::model::{KeyPrincipal, PrincipalKind, Refusal};

    fn refusal(principal: &str, action: &str) -> Refusal {
        Refusal {
            principal: principal.into(),
            action: action.into(),
            resource: "*".into(),
            decision: "implicitDeny".into(),
            reason: "because".into(),
            count: 1,
            last_seen: "2026-11-21T00:00:00Z".into(),
        }
    }

    #[test]
    fn a_key_is_known_by_its_shape_until_iam_names_it() {
        assert_eq!(shape("console"), PrincipalKind::Console);
        assert_eq!(shape("ASIAEXAMPLE"), PrincipalKind::Session);
        assert_eq!(shape("test"), PrincipalKind::Root);
        assert_eq!(shape("AKIAUNKNOWN"), PrincipalKind::Root);
    }

    #[test]
    fn a_refusal_explains_the_same_principal_and_the_action_named_as_the_operation() {
        let user = KeyPrincipal {
            key: "AKIA1".into(),
            kind: PrincipalKind::User,
            name: Some("app".into()),
            arn: Some("arn:aws:iam::1:user/app".into()),
        };
        let log = [
            refusal("arn:aws:iam::1:user/app", "sqs:SendMessage"),
            refusal("arn:aws:iam::1:user/other", "sqs:SendMessage"),
            refusal("arn:aws:iam::1:user/app", "sqs:ListQueues"),
            refusal("arn:aws:sts::1:assumed-role/r/s", "sqs:SendMessage"),
        ];
        let (found, by_service) = explaining(&log, &user, "SendMessage", "sqs");
        assert_eq!((found.len(), by_service), (1, false));
        assert_eq!(found[0].principal, "arn:aws:iam::1:user/app");
        let session = KeyPrincipal {
            key: "ASIA1".into(),
            kind: PrincipalKind::Session,
            name: None,
            arn: None,
        };
        assert_eq!(explaining(&log, &session, "SendMessage", "sqs").0.len(), 1);
        // No row by the operation's name: the principal's refusals of the service, said to be matched so.
        let (other, by_service) = explaining(&log, &user, "ReceiveMessage", "sqs");
        assert_eq!((other.len(), by_service), (2, true));
        let root = KeyPrincipal {
            kind: PrincipalKind::Root,
            ..session
        };
        assert!(explaining(&log, &root, "SendMessage", "sqs").0.is_empty());
    }
}
