//! doze's own diagnostics, not AWS operations: the **access log** (every authorization question IAM answered in soft or enforce mode:
//! who, what, on what, the verdict and why) and the **policy generator** that turns what one principal was seen doing into a
//! least-privilege policy document. Run a workload in soft mode, ask for the policy, commit it.
//!
//! They are not in the IAM model, so the Query endpoint (which routes by the model) cannot carry them. They are served as two
//! addresses of IAM's own on the gateway, unsigned like a Lambda function URL:
//!
//! - `GET /_doze/iam/DozeAccessLog` (`?Reset=true` clears it)
//! - `GET /_doze/iam/DozeGeneratePolicy?Principal=<arn>&ScopeToResources=true`
//!
//! The answers are JSON. The log is kept in memory only (it is a diagnostic of the current run), holds at most [`MAX_ENTRIES`]
//! distinct (principal, action, resource) rows (the least recently seen goes when a new one would pass that), and is not made at
//! all until the first question is asked, which never happens with IAM off.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};

/// The most distinct rows kept.
pub(crate) const MAX_ENTRIES: usize = 2000;

/// One authorization question and its answer, as the authorizer saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    /// How the caller is named: a user's ARN, a role session's ARN, or the account root.
    pub(crate) principal: String,
    /// The role behind a role session, so a filter on the role's ARN finds its sessions.
    pub(crate) role: Option<String>,
    pub(crate) action: String,
    /// The ARN acted on; `*` when the service named none (an action on no resource, or one it did not resolve).
    pub(crate) resource: String,
    /// `allowed`, `implicitDeny` or `explicitDeny`.
    pub(crate) decision: &'static str,
    /// Why: the statement that allowed, or the refusal's reason.
    pub(crate) reason: String,
    pub(crate) at_ms: i64,
}

#[derive(Debug)]
struct Row {
    seen: Seen,
    count: u64,
    first_ms: i64,
}

/// The log: one row per (principal, action, resource), with how often and when.
#[derive(Debug, Default)]
pub(crate) struct AccessLog {
    rows: HashMap<(String, String, String), Row>,
    /// Rows pushed out by newer ones since the last reset.
    evicted: u64,
}

impl AccessLog {
    pub(crate) fn record(&mut self, seen: Seen) {
        let key = (
            seen.principal.clone(),
            seen.action.clone(),
            seen.resource.clone(),
        );
        if let Some(row) = self.rows.get_mut(&key) {
            row.count += 1;
            // The latest verdict, and the reason that goes with it.
            row.seen = seen;
            return;
        }
        if self.rows.len() >= MAX_ENTRIES
            && let Some(oldest) = self
                .rows
                .iter()
                .min_by_key(|(_, r)| r.seen.at_ms)
                .map(|(k, _)| k.clone())
        {
            self.rows.remove(&oldest);
            self.evicted += 1;
        }
        let first_ms = seen.at_ms;
        self.rows.insert(
            key,
            Row {
                seen,
                count: 1,
                first_ms,
            },
        );
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The rows, by principal, action and resource.
    fn sorted(&self) -> Vec<&Row> {
        let mut rows: Vec<&Row> = self.rows.values().collect();
        rows.sort_by(|a, b| {
            (&a.seen.principal, &a.seen.action, &a.seen.resource).cmp(&(
                &b.seen.principal,
                &b.seen.action,
                &b.seen.resource,
            ))
        });
        rows
    }

    /// `DozeAccessLog`'s answer.
    pub(crate) fn entries(&self, mode: &str) -> Value {
        let entries: Vec<Value> = self
            .sorted()
            .into_iter()
            .map(|r| {
                json!({
                    "Principal": r.seen.principal,
                    "Action": r.seen.action,
                    "Resource": r.seen.resource,
                    "Decision": r.seen.decision,
                    "Reason": r.seen.reason,
                    "Count": r.count,
                    "FirstSeen": aws_core::time::format_iso8601(r.first_ms / 1000),
                    "LastSeen": aws_core::time::format_iso8601(r.seen.at_ms / 1000),
                })
            })
            .collect();
        json!({"Mode": mode, "Entries": entries, "Evicted": self.evicted})
    }

    /// `DozeGeneratePolicy`'s answer: one `Allow` granting every action `principal` (or anyone, when `None`) was seen asking for,
    /// on `*`, or with `scoped` one statement per resource. A row on `*` stays on `*` and is counted in `UnresolvedResources`, so a
    /// scoped policy never looks narrower than it is.
    pub(crate) fn generate(&self, principal: Option<&str>, scoped: bool) -> Option<Value> {
        let mut by_resource: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let mut unresolved = 0_u64;
        for row in self.rows.values() {
            let s = &row.seen;
            if let Some(want) = principal
                && s.principal != want
                && s.role.as_deref() != Some(want)
            {
                continue;
            }
            let resource = if scoped { s.resource.as_str() } else { "*" };
            if scoped && resource == "*" {
                unresolved += 1;
            }
            by_resource.entry(resource).or_default().insert(&s.action);
        }
        if by_resource.is_empty() {
            return None;
        }
        let statements: Vec<Value> = by_resource
            .iter()
            .map(|(resource, actions)| {
                json!({"Effect": "Allow", "Action": actions.iter().collect::<Vec<_>>(), "Resource": resource})
            })
            .collect();
        Some(json!({
            "PolicyDocument": {"Version": "2012-10-17", "Statement": statements},
            "StatementCount": by_resource.len(),
            "UnresolvedResources": unresolved,
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn seen(principal: &str, action: &str, resource: &str, at_ms: i64) -> Seen {
        Seen {
            principal: principal.into(),
            role: None,
            action: action.into(),
            resource: resource.into(),
            decision: "allowed",
            reason: String::new(),
            at_ms,
        }
    }

    #[test]
    fn a_row_counts_repeats_and_keeps_the_latest_verdict() {
        let mut log = AccessLog::default();
        log.record(seen("u", "sqs:SendMessage", "q", 1000));
        let mut denied = seen("u", "sqs:SendMessage", "q", 2000);
        denied.decision = "implicitDeny";
        log.record(denied);
        let out = log.entries("soft");
        assert_eq!(out["Entries"].as_array().unwrap().len(), 1);
        assert_eq!(out["Entries"][0]["Count"], 2);
        assert_eq!(out["Entries"][0]["Decision"], "implicitDeny");
        assert_eq!(out["Entries"][0]["FirstSeen"], "1970-01-01T00:00:01.000Z");
    }

    #[test]
    fn the_log_is_bounded_and_drops_the_least_recently_seen() {
        let mut log = AccessLog::default();
        for i in 0..MAX_ENTRIES + 5 {
            log.record(seen(
                "u",
                "s3:GetObject",
                &format!("r{i}"),
                i64::try_from(i).unwrap(),
            ));
        }
        assert_eq!(log.len(), MAX_ENTRIES);
        let out = log.entries("soft");
        assert_eq!(out["Evicted"], 5);
        assert!(
            !out["Entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["Resource"] == "r0")
        );
    }

    #[test]
    fn a_generated_policy_grants_what_was_used_and_names_what_it_could_not_scope() {
        let mut log = AccessLog::default();
        log.record(seen("alice", "sqs:SendMessage", "arn:q", 1));
        log.record(seen("alice", "sqs:ReceiveMessage", "arn:q", 2));
        log.record(seen("alice", "sqs:ListQueues", "*", 3));
        log.record(seen("bob", "s3:GetObject", "arn:b/k", 4));
        let mut session = seen("arn:sts/assumed-role/r/s", "kms:Decrypt", "arn:k", 5);
        session.role = Some("arn:role/r".into());
        log.record(session);

        let all = log.generate(Some("alice"), false).unwrap();
        assert_eq!(
            all["PolicyDocument"]["Statement"],
            json!([{"Effect": "Allow", "Action": ["sqs:ListQueues", "sqs:ReceiveMessage", "sqs:SendMessage"], "Resource": "*"}])
        );
        let scoped = log.generate(Some("alice"), true).unwrap();
        assert_eq!(scoped["StatementCount"], 2);
        assert_eq!(scoped["UnresolvedResources"], 1);
        assert_eq!(
            scoped["PolicyDocument"]["Statement"][1],
            json!({"Effect": "Allow", "Action": ["sqs:ReceiveMessage", "sqs:SendMessage"], "Resource": "arn:q"})
        );
        // A role's sessions are found by the role's ARN.
        let role = log.generate(Some("arn:role/r"), true).unwrap();
        assert_eq!(
            role["PolicyDocument"]["Statement"][0]["Action"],
            json!(["kms:Decrypt"])
        );
        assert!(log.generate(Some("nobody"), false).is_none());
        // The document is one IAM accepts.
        let text = all["PolicyDocument"].to_string();
        aws_iam_policy::check(&text, aws_iam_policy::Kind::Identity).unwrap();
    }
}
