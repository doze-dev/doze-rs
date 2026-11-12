//! Aliases: `CreateAlias`, `UpdateAlias`, `DeleteAlias`, `ListAliases`. Words recorded 2026-11-09 unless marked.

use aws_model_kms::keys::{
    AliasListEntry, CreateAliasRequest as Create, DeleteAliasRequest as Delete,
    ListAliasesRequest as List, ListAliasesResponse as Listed, UpdateAliasRequest as Update,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, validation};
use crate::find::{alias_arn, not_found_alias, not_found_key};
use crate::keys::{invalid_state, seconds};
use crate::model::{Alias, State};

const LIST_PAGE: usize = 100;

fn check_name(call: &Call, name: &str) -> Result<(), ApiError> {
    if !name.starts_with("alias/") {
        return Err(validation(
            "Alias must start with the prefix \"alias/\". Please see https://docs.aws.amazon.com/kms/latest/developerguide/kms-alias.html",
        ));
    }
    if name.starts_with("alias/aws/") {
        // (memory)
        return Err(ApiError::modeled(
            "NotAuthorizedException",
            format!(
                "Alias {} is reserved: names that start with alias/aws/ belong to AWS.",
                alias_arn(call, name)
            ),
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/_-:".contains(&b))
    {
        // (memory)
        return Err(validation(format!(
            "1 validation error detected: Value '{name}' at 'aliasName' failed to satisfy constraint: Member must satisfy regular expression pattern: ^[a-zA-Z0-9/_-]+$"
        )));
    }
    Ok(())
}

impl Core {
    /// The id of a key an alias may point at.
    fn alias_target(&self, call: &Call, given: &str) -> Result<String, ApiError> {
        let key = self.key(call, given)?;
        if key.state == State::PendingDeletion {
            return Err(invalid_state(format!("{} is pending deletion.", key.arn)));
        }
        Ok(key.id.clone())
    }

    pub(crate) fn create_alias(&mut self, call: &Call) -> Reply {
        let v = request::<Create>(call)?;
        let name = v.get(Create::ALIAS_NAME).unwrap_or_default();
        check_name(call, name)?;
        if self.aliases.get(name).is_some() {
            return Err(ApiError::modeled(
                "AlreadyExistsException",
                format!(
                    "An alias with the name {} already exists",
                    alias_arn(call, name)
                ),
            ));
        }
        let key_id = self.alias_target(call, v.get(Create::TARGET_KEY_ID).unwrap_or_default())?;
        let now = self.now_ms();
        self.aliases.insert(
            name.to_owned(),
            Alias {
                name: name.to_owned(),
                key_id,
                created_ms: now,
                updated_ms: now,
            },
        );
        empty()
    }

    pub(crate) fn update_alias(&mut self, call: &Call) -> Reply {
        let v = request::<Update>(call)?;
        let name = v.get(Update::ALIAS_NAME).unwrap_or_default();
        if self.aliases.get(name).is_none() {
            return Err(not_found_alias(call, name));
        }
        let key_id = self.alias_target(call, v.get(Update::TARGET_KEY_ID).unwrap_or_default())?;
        let now = self.now_ms();
        if let Some(a) = self.aliases.get_mut(name) {
            a.key_id = key_id;
            a.updated_ms = now;
        }
        empty()
    }

    pub(crate) fn delete_alias(&mut self, call: &Call) -> Reply {
        let v = request::<Delete>(call)?;
        let name = v.get(Delete::ALIAS_NAME).unwrap_or_default();
        self.aliases
            .remove(name)
            .ok_or_else(|| not_found_alias(call, name))?;
        empty()
    }

    pub(crate) fn list_aliases(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::LIMIT)
            .map_or(LIST_PAGE, |n| usize::try_from(n).unwrap_or(LIST_PAGE));
        let after = v.get(List::MARKER).unwrap_or_default();
        let only = match v.get(List::KEY_ID) {
            Some(given) => Some(
                self.locate(call, given)
                    .map_err(|_| not_found_key(call, given))?,
            ),
            None => None,
        };
        let mut rows: Vec<&Alias> = self
            .aliases
            .iter()
            .map(|(_, a)| a)
            .filter(|a| a.name.as_str() > after && only.as_ref().is_none_or(|id| *id == a.key_id))
            .collect();
        let more = rows.len() > limit;
        rows.truncate(limit);
        let marker = more.then(|| rows.last().map(|a| a.name.clone())).flatten();
        reply::<Listed>(|o| {
            o.put_list(Listed::ALIASES, |l| {
                for a in &rows {
                    l.push(|e: &mut Object<'_, AliasListEntry>| {
                        e.put(AliasListEntry::ALIAS_NAME, &a.name);
                        e.put(AliasListEntry::ALIAS_ARN, &alias_arn(call, &a.name));
                        e.put(AliasListEntry::TARGET_KEY_ID, &a.key_id);
                        e.put_time(AliasListEntry::CREATION_DATE, seconds(a.created_ms));
                        e.put_time(AliasListEntry::LAST_UPDATED_DATE, seconds(a.updated_ms));
                    });
                }
            });
            o.put_bool(Listed::TRUNCATED, more);
            if let Some(m) = &marker {
                o.put(Listed::NEXT_MARKER, m);
            }
        })
    }
}
