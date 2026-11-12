//! Access keys: a user's long-term credentials.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::entity::new_id;
use crate::errors::{limit, no_such};
use crate::model::{AccessKey, Kind};
use crate::req::{Req, page, secs};

const MAX_KEYS: usize = 2;

/// 40 characters of a key's secret.
fn new_secret() -> Result<String, ApiError> {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    (0..40)
        .map(|_| {
            aws_seal::random_below(SET.len())
                .and_then(|i| SET.get(i))
                .map(|b| char::from(*b))
        })
        .collect::<Option<String>>()
        .ok_or_else(|| ApiError::internal("no randomness"))
}

impl Core {
    /// The user a key call is about: the one named, or the one whose key signed the call.
    fn key_user(&self, r: &Req<'_>) -> Result<String, ApiError> {
        if let Some(name) = r.opt("UserName") {
            self.entity(Kind::User, name)?;
            return Ok(name.to_owned());
        }
        self.keys
            .get(&r.call.caller.access_key_id)
            .map(|k| k.user.clone())
            .ok_or_else(|| no_such("user", ""))
    }

    pub(crate) fn create_access_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.key_user(r)?;
        if self.keys.iter().filter(|(_, k)| k.user == user).count() >= MAX_KEYS {
            return Err(limit("Cannot exceed quota for AccessKeysPerUser: 2"));
        }
        let id = new_id("AKIA")?;
        let key = AccessKey {
            id: id.clone(),
            secret: new_secret()?,
            user: user.clone(),
            active: true,
            created_ms: self.now_ms(),
            last_used: None,
        };
        let shown = json!({"UserName": user, "AccessKeyId": id, "Status": "Active", "SecretAccessKey": key.secret, "CreateDate": secs(key.created_ms)});
        self.keys.insert(id, key);
        Ok(json!({"AccessKey": shown}))
    }

    pub(crate) fn list_access_keys(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let user = self.key_user(r)?;
        let items = self
            .keys
            .iter()
            .filter(|(_, k)| k.user == user)
            .map(|(_, k)| json!({"UserName": k.user, "AccessKeyId": k.id, "Status": status(k), "CreateDate": secs(k.created_ms)}))
            .collect();
        page(r, items, "AccessKeyMetadata")
    }

    fn own_key(&mut self, r: &Req<'_>) -> Result<&mut AccessKey, ApiError> {
        let user = self.key_user(r)?;
        let id = r.s("AccessKeyId");
        match self.keys.get_mut(id) {
            Some(k) if k.user == user => Ok(k),
            _ => Err(no_such_key(id)),
        }
    }

    pub(crate) fn update_access_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let active = r.s("Status") == "Active";
        self.own_key(r)?.active = active;
        Ok(Value::Null)
    }

    pub(crate) fn delete_access_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let id = self.own_key(r)?.id.clone();
        self.keys.remove(&id);
        Ok(Value::Null)
    }

    pub(crate) fn get_access_key_last_used(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let k = self
            .keys
            .get(r.s("AccessKeyId"))
            .ok_or_else(|| no_such_key(r.s("AccessKeyId")))?;
        let used = match &k.last_used {
            Some((at, service, region)) => {
                json!({"LastUsedDate": secs(*at), "ServiceName": service, "Region": region})
            }
            None => json!({"ServiceName": "N/A", "Region": "N/A"}),
        };
        Ok(json!({"UserName": k.user, "AccessKeyLastUsed": used}))
    }
}

fn status(k: &AccessKey) -> &'static str {
    if k.active { "Active" } else { "Inactive" }
}

fn no_such_key(id: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("The Access Key with id {id} cannot be found."),
    )
}
