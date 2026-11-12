//! Grants: `CreateGrant`, `ListGrants`, `ListRetirableGrants`, `RevokeGrant`, `RetireGrant`. A grant is kept and listed as made; it
//! is not evaluated (it lets nobody do anything) until the IAM stage can read the principal of a request. The refusals' words are
//! not recorded.

use std::collections::BTreeMap;

use aws_model_kms::keys::{
    CreateGrantRequest as Create, CreateGrantResponse as Created, GrantConstraints, GrantListEntry,
    ListGrantsRequest as List, ListGrantsResponse as Listed,
    ListRetirableGrantsRequest as Retirable, RetireGrantRequest as Retire,
    RevokeGrantRequest as Revoke,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::find::not_found_key;
use crate::keys::{invalid_state, no_randomness, seconds};
use crate::model::{Grant, Key, State};

const OPERATIONS: [&str; 14] = [
    "Decrypt",
    "Encrypt",
    "GenerateDataKey",
    "GenerateDataKeyWithoutPlaintext",
    "ReEncryptFrom",
    "ReEncryptTo",
    "Sign",
    "Verify",
    "GetPublicKey",
    "CreateGrant",
    "RetireGrant",
    "DescribeKey",
    "GenerateDataKeyPair",
    "GenerateDataKeyPairWithoutPlaintext",
];
const LIST_PAGE: usize = 50;

fn map_of<'a>(pairs: Option<impl Iterator<Item = (&'a str, &'a str)>>) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn grant_not_found(id: &str) -> ApiError {
    ApiError::modeled("NotFoundException", format!("Grant ID {id} was not found"))
}

fn show(e: &mut Object<'_, GrantListEntry>, key: &Key, g: &Grant, account: &str) {
    e.put(GrantListEntry::KEY_ID, &key.arn);
    e.put(GrantListEntry::GRANT_ID, &g.id);
    if let Some(n) = &g.name {
        e.put(GrantListEntry::NAME, n);
    }
    e.put_time(GrantListEntry::CREATION_DATE, seconds(g.created_ms));
    e.put(GrantListEntry::GRANTEE_PRINCIPAL, &g.grantee);
    if let Some(r) = &g.retiring {
        e.put(GrantListEntry::RETIRING_PRINCIPAL, r);
    }
    e.put(
        GrantListEntry::ISSUING_ACCOUNT,
        &format!("arn:aws:iam::{account}:root"),
    );
    e.put_list(GrantListEntry::OPERATIONS, |l| {
        for o in &g.operations {
            l.push(o);
        }
    });
    if !g.context_subset.is_empty() || !g.context_equals.is_empty() {
        e.put_object(
            GrantListEntry::CONSTRAINTS,
            |c: &mut Object<'_, GrantConstraints>| {
                for (member, map) in [
                    (
                        GrantConstraints::ENCRYPTION_CONTEXT_SUBSET,
                        &g.context_subset,
                    ),
                    (
                        GrantConstraints::ENCRYPTION_CONTEXT_EQUALS,
                        &g.context_equals,
                    ),
                ] {
                    if !map.is_empty() {
                        c.put_map(member, |m| {
                            for (k, v) in map {
                                m.insert(k, v);
                            }
                        });
                    }
                }
            },
        );
    }
}

/// The grant a `CreateGrant` asks for.
fn grant_of(v: &aws_schema::view::View<'_, Create>, now: i64) -> Result<Grant, ApiError> {
    let operations: Vec<String> = v
        .get(Create::OPERATIONS)
        .map(|o| o.map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(bad) = operations
        .iter()
        .find(|o| !OPERATIONS.contains(&o.as_str()))
    {
        return Err(validation(format!("Grant operation {bad} is not valid.")));
    }
    let grantee = v
        .get(Create::GRANTEE_PRINCIPAL)
        .or(v.get(Create::GRANTEE_SERVICE_PRINCIPAL))
        .ok_or_else(|| validation("A grant needs a grantee principal."))?;
    let constraints = v.get(Create::CONSTRAINTS);
    let grant = Grant {
        id: aws_core::hex::encode(&aws_seal::random_bytes::<32>().ok_or_else(no_randomness)?),
        token: aws_core::hex::encode(&aws_seal::random_bytes::<48>().ok_or_else(no_randomness)?),
        name: v.get(Create::NAME).map(str::to_owned),
        grantee: grantee.to_owned(),
        retiring: v
            .get(Create::RETIRING_PRINCIPAL)
            .or(v.get(Create::RETIRING_SERVICE_PRINCIPAL))
            .map(str::to_owned),
        operations,
        context_subset: map_of(
            constraints
                .as_ref()
                .and_then(|c| c.get(GrantConstraints::ENCRYPTION_CONTEXT_SUBSET)),
        ),
        context_equals: map_of(
            constraints
                .as_ref()
                .and_then(|c| c.get(GrantConstraints::ENCRYPTION_CONTEXT_EQUALS)),
        ),
        created_ms: now,
    };

    Ok(grant)
}

impl Core {
    pub(crate) fn create_grant(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let grant = grant_of(&v, self.now_ms())?;
        let id = self.locate(call, v.get(Create::KEY_ID).unwrap_or_default())?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        match key.state {
            State::PendingDeletion => {
                return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
            }
            State::PendingImport => {
                return Err(ApiError::modeled(
                    "KMSInvalidStateException",
                    format!("{} is pending import.", key.arn),
                ));
            }
            State::Disabled => {
                return Err(ApiError::modeled(
                    "DisabledException",
                    format!("{} is disabled.", key.arn),
                ));
            }
            State::Enabled => {}
        }
        // A grant with the same name on the same key is the same grant.
        if let Some(name) = &grant.name
            && let Some(same) = key.grants.iter().find(|g| g.name.as_ref() == Some(name))
        {
            let (gid, token) = (same.id.clone(), same.token.clone());
            return reply::<Created>(|o| {
                o.put(Created::GRANT_TOKEN, &token);
                o.put(Created::GRANT_ID, &gid);
            });
        }
        let (gid, token) = (grant.id.clone(), grant.token.clone());
        key.grants.push(grant);
        reply::<Created>(|o| {
            o.put(Created::GRANT_TOKEN, &token);
            o.put(Created::GRANT_ID, &gid);
        })
    }

    pub(crate) fn list_grants(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::LIMIT)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let after = v.get(List::MARKER).unwrap_or_default();
        let key = self.key(call, v.get(List::KEY_ID).unwrap_or_default())?;
        let (want_id, want_grantee) = (
            v.get(List::GRANT_ID),
            v.get(List::GRANTEE_PRINCIPAL)
                .or(v.get(List::GRANTEE_SERVICE_PRINCIPAL)),
        );
        let mut rows: Vec<&Grant> = key
            .grants
            .iter()
            .filter(|g| {
                g.id.as_str() > after
                    && want_id.is_none_or(|i| i == g.id)
                    && want_grantee.is_none_or(|p| p == g.grantee)
            })
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        self.page_grants(call, key, &rows, limit)
    }

    fn page_grants(&self, call: &Call, key: &Key, rows: &[&Grant], limit: usize) -> Reply {
        let more = rows.len() > limit;
        let page = rows.iter().take(limit);
        let marker = more
            .then(|| rows.get(limit.saturating_sub(1)).map(|g| g.id.clone()))
            .flatten();
        reply::<Listed>(|o| {
            o.put_list(Listed::GRANTS, |l| {
                for g in page {
                    l.push(|e| show(e, key, g, &call.caller.account_id));
                }
            });
            o.put_bool(Listed::TRUNCATED, more);
            if let Some(m) = &marker {
                o.put(Listed::NEXT_MARKER, m);
            }
        })
    }

    pub(crate) fn list_retirable_grants(&mut self, call: &Call) -> Reply {
        let v = request::<Retirable>(call)?;
        let limit = v
            .get(Retirable::LIMIT)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let who = v
            .get(Retirable::RETIRING_PRINCIPAL)
            .or(v.get(Retirable::RETIRING_SERVICE_PRINCIPAL))
            .ok_or_else(|| validation("A retiring principal is required."))?;
        let mut rows: Vec<(&Key, &Grant)> = self
            .keys
            .iter()
            .flat_map(|(_, k)| k.grants.iter().map(move |g| (k, g)))
            .filter(|(_, g)| g.retiring.as_deref() == Some(who))
            .collect();
        rows.sort_by(|a, b| a.1.id.cmp(&b.1.id));
        let page: Vec<_> = rows.into_iter().take(limit).collect();
        reply::<Listed>(|o| {
            o.put_list(Listed::GRANTS, |l| {
                for (k, g) in &page {
                    l.push(|e| show(e, k, g, &call.caller.account_id));
                }
            });
            o.put_bool(Listed::TRUNCATED, false);
        })
    }

    pub(crate) fn revoke_grant(&mut self, call: &Call) -> Reply {
        let v = request::<Revoke>(call)?;
        let id = self.locate(call, v.get(Revoke::KEY_ID).unwrap_or_default())?;
        let gid = v.get(Revoke::GRANT_ID).unwrap_or_default();
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        let at = key
            .grants
            .iter()
            .position(|g| g.id == gid)
            .ok_or_else(|| grant_not_found(gid))?;
        key.grants.remove(at);
        empty()
    }

    pub(crate) fn retire_grant(&mut self, call: &Call) -> Reply {
        let v = request::<Retire>(call)?;
        if let Some(token) = v.get(Retire::GRANT_TOKEN) {
            let owner = self
                .keys
                .iter()
                .find(|(_, k)| k.grants.iter().any(|g| g.token == token))
                .map(|(id, _)| id.to_owned());
            let key = owner
                .and_then(|id| self.keys.get_mut(&id))
                .ok_or_else(|| grant_not_found(token))?;
            key.grants.retain(|g| g.token != token);
            return empty();
        }
        let id = self.locate(
            call,
            v.get(Retire::KEY_ID)
                .ok_or_else(|| validation("Give a grant token, or a key and a grant id."))?,
        )?;
        let gid = v
            .get(Retire::GRANT_ID)
            .ok_or_else(|| validation("Give a grant token, or a key and a grant id."))?;
        let key = self
            .keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))?;
        let at = key
            .grants
            .iter()
            .position(|g| g.id == gid)
            .ok_or_else(|| grant_not_found(gid))?;
        key.grants.remove(at);
        empty()
    }
}
