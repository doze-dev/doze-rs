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

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};

/// The most distinct rows kept.
pub(crate) const MAX_ENTRIES: usize = 2000;

/// No row: the end of the recency list.
const NONE: u32 = u32::MAX;

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
    /// Why: the statement that allowed, or the refusal's reason (a fixed sentence, for the account root, costs nothing to keep).
    pub(crate) reason: Cow<'static, str>,
    /// The identity policy that held the Allow statement that let it through, with its kind: told to the bus for the Live record
    /// ([`aws_peers::Peers::allowed_by`]) and not kept in the log.
    pub(crate) by: Option<(String, &'static str)>,
    pub(crate) at_ms: i64,
}

/// What a question asked, borrowed: the three parts a row is keyed by, and the role behind them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Asked<'a> {
    pub(crate) principal: &'a str,
    pub(crate) role: Option<&'a str>,
    pub(crate) action: &'a str,
    pub(crate) resource: &'a str,
}

/// How it was answered.
#[derive(Clone, Debug)]
pub(crate) struct Verdict {
    pub(crate) decision: &'static str,
    pub(crate) reason: Cow<'static, str>,
    pub(crate) at_ms: i64,
}

#[derive(Debug)]
struct Row {
    /// `principal`, `action` and `resource`, joined by NUL (which no ARN or action holds): one allocation a row, shared with the index.
    key: Arc<str>,
    role: Option<String>,
    verdict: Verdict,
    count: u64,
    first_ms: i64,
    /// The rows seen just before and just after this one: the least recently seen is the list's head.
    prev: u32,
    next: u32,
}

impl Row {
    fn part(&self, n: usize) -> &str {
        self.key.split('\0').nth(n).unwrap_or_default()
    }
    fn principal(&self) -> &str {
        self.part(0)
    }
    fn action(&self) -> &str {
        self.part(1)
    }
    fn resource(&self) -> &str {
        self.part(2)
    }
}

/// The log: one row per (principal, action, resource), with how often and when. A repeat is found without allocating, and the
/// least recently seen row is dropped in constant time (the rows are a list in the order they were last seen).
#[derive(Debug)]
pub(crate) struct AccessLog {
    index: HashMap<Arc<str>, u32>,
    rows: Vec<Row>,
    head: u32,
    tail: u32,
    /// Where a key is joined to be looked up; kept, so a lookup does not allocate.
    scratch: String,
    /// The account the root ARN below is for, and the ARN.
    root: (String, String),
    /// Rows pushed out by newer ones since the last reset.
    evicted: u64,
}

impl Default for AccessLog {
    fn default() -> Self {
        Self {
            index: HashMap::new(),
            rows: Vec::new(),
            head: NONE,
            tail: NONE,
            scratch: String::new(),
            root: (String::new(), String::new()),
            evicted: 0,
        }
    }
}

impl AccessLog {
    pub(crate) fn record(&mut self, seen: Seen) {
        let asked = Asked {
            principal: &seen.principal,
            role: seen.role.as_deref(),
            action: &seen.action,
            resource: &seen.resource,
        };
        let verdict = Verdict {
            decision: seen.decision,
            reason: seen.reason.clone(),
            at_ms: seen.at_ms,
        };
        self.note(asked, verdict);
    }

    /// Records one question: a repeat counts and takes the latest verdict; a new row pushes out the least recently seen when full.
    pub(crate) fn note(&mut self, asked: Asked<'_>, verdict: Verdict) {
        self.scratch.clear();
        for (i, part) in [asked.principal, asked.action, asked.resource]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                self.scratch.push('\0');
            }
            self.scratch.push_str(part);
        }
        if let Some(&at) = self.index.get(self.scratch.as_str()) {
            if let Some(row) = self.rows.get_mut(at as usize) {
                row.count += 1;
                // The latest verdict, and the reason that goes with it.
                if row.role.as_deref() != asked.role {
                    row.role = asked.role.map(str::to_owned);
                }
                row.verdict = verdict;
            }
            self.unlink(at);
            self.push_back(at);
            return;
        }
        let key: Arc<str> = Arc::from(self.scratch.as_str());
        let row = Row {
            key: Arc::clone(&key),
            role: asked.role.map(str::to_owned),
            first_ms: verdict.at_ms,
            verdict,
            count: 1,
            prev: NONE,
            next: NONE,
        };
        let at = if self.rows.len() >= MAX_ENTRIES && self.head != NONE {
            let oldest = self.head;
            self.unlink(oldest);
            if let Some(slot) = self.rows.get_mut(oldest as usize) {
                self.index.remove(&slot.key);
                *slot = row;
            }
            self.evicted += 1;
            oldest
        } else {
            self.rows.push(row);
            u32::try_from(self.rows.len() - 1).unwrap_or(NONE)
        };
        self.index.insert(key, at);
        self.push_back(at);
    }

    /// The account root's questions (`(action, resource)`), all allowed. Its ARN is made once per account, and its reason is a
    /// fixed sentence: a repeat allocates nothing.
    pub(crate) fn note_root<'a>(
        &mut self,
        account: &str,
        needs: impl Iterator<Item = (&'a str, &'a str)>,
        at_ms: i64,
    ) {
        if self.root.0 != account {
            self.root = (account.to_owned(), format!("arn:aws:iam::{account}:root"));
        }
        let root = std::mem::take(&mut self.root.1);
        for (action, resource) in needs {
            let asked = Asked {
                principal: &root,
                role: None,
                action,
                resource,
            };
            let verdict = Verdict {
                decision: "allowed",
                reason: Cow::Borrowed("the account root is allowed everything"),
                at_ms,
            };
            self.note(asked, verdict);
        }
        self.root.1 = root;
    }

    fn unlink(&mut self, at: u32) {
        let Some((prev, next)) = self.rows.get(at as usize).map(|r| (r.prev, r.next)) else {
            return;
        };
        match self.rows.get_mut(prev as usize) {
            Some(p) => p.next = next,
            None => self.head = next,
        }
        match self.rows.get_mut(next as usize) {
            Some(n) => n.prev = prev,
            None => self.tail = prev,
        }
    }

    fn push_back(&mut self, at: u32) {
        let tail = self.tail;
        if let Some(r) = self.rows.get_mut(at as usize) {
            r.prev = tail;
            r.next = NONE;
        }
        match self.rows.get_mut(tail as usize) {
            Some(t) => t.next = at,
            None => self.head = at,
        }
        self.tail = at;
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The rows, by principal, action and resource.
    fn sorted(&self) -> Vec<&Row> {
        let mut rows: Vec<&Row> = self.rows.iter().collect();
        rows.sort_by(|a, b| {
            (a.principal(), a.action(), a.resource()).cmp(&(
                b.principal(),
                b.action(),
                b.resource(),
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
                    "Principal": r.principal(),
                    "Action": r.action(),
                    "Resource": r.resource(),
                    "Decision": r.verdict.decision,
                    "Reason": r.verdict.reason,
                    "Count": r.count,
                    "FirstSeen": aws_core::time::format_iso8601(r.first_ms / 1000),
                    "LastSeen": aws_core::time::format_iso8601(r.verdict.at_ms / 1000),
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
        for row in &self.rows {
            if let Some(want) = principal
                && row.principal() != want
                && row.role.as_deref() != Some(want)
            {
                continue;
            }
            let resource = if scoped { row.resource() } else { "*" };
            if scoped && resource == "*" {
                unresolved += 1;
            }
            by_resource
                .entry(resource)
                .or_default()
                .insert(row.action());
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
mod tests;
