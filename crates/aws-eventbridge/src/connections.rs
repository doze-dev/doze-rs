//! Connections: the credentials an API destination calls with. Basic, API key and OAuth client credentials are stored. The secrets are
//! kept in the record and never returned: `DescribeConnection` answers without a password, key value or client secret, and without
//! the value of any parameter marked `IsValueSecret`, as AWS does. Nothing calls out with them yet (see `destinations.rs`).
//!
//! The states a call answers (`CREATING`, `UPDATING`, `DELETING`, `DEAUTHORIZING`) are the ones AWS documents for the transition;
//! a described connection is `AUTHORIZED` or `DEAUTHORIZED`. The wording of refusals is from memory.

use aws_wire::ApiError;
use aws_wire::RequestId;
use serde_json::{Map, Value};

use crate::core::Core;
use crate::model::{Connection, key};
use crate::names::with_uuid;
use crate::page::{page, with_token};
use crate::req::{Req, exists, invalid, not_found};
use crate::util::{Out, secs};

/// The member of `AuthParameters` each authorization type must carry.
fn block_of(auth_type: &str) -> &'static str {
    match auth_type {
        "BASIC" => "BasicAuthParameters",
        "OAUTH_CLIENT_CREDENTIALS" => "OAuthParameters",
        _ => "ApiKeyAuthParameters",
    }
}

impl Core {
    fn connection(&self, r: &Req<'_>, name: &str) -> Result<&Connection, ApiError> {
        self.connections
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| not_found(format!("Connection '{name}' does not exist.")))
    }

    pub(crate) fn create_connection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        if self.connections.get(&key(r.account(), &[name])).is_some() {
            return Err(exists(format!("Connection {name} already exists.")));
        }
        let auth_type = r.s("AuthorizationType");
        let params = r.v.get("AuthParameters").cloned().unwrap_or(Value::Null);
        if params.get(block_of(auth_type)).is_none() {
            return Err(invalid(format!(
                "AuthParameters must contain {} for AuthorizationType {auth_type}.",
                block_of(auth_type)
            )));
        }
        let uuid = RequestId::new().to_string();
        let now = self.now_ms();
        let c = Connection {
            account: r.account().to_owned(),
            name: name.to_owned(),
            arn: with_uuid(r.region(), r.account(), "connection", name, &uuid),
            description: r.opt("Description").map(str::to_owned),
            auth_type: auth_type.to_owned(),
            auth_parameters: params.to_string(),
            connectivity: r.json_text("InvocationConnectivityParameters"),
            kms_key: r.opt("KmsKeyIdentifier").map(str::to_owned),
            state: "AUTHORIZED".to_owned(),
            // AWS keeps the secret in Secrets Manager under this name; here it is in the record, and only the name is made.
            secret_arn: format!(
                "arn:aws:secretsmanager:{}:{}:secret:events!connection/{name}/{uuid}-{}",
                r.region(),
                r.account(),
                uuid.get(..6).unwrap_or_default()
            ),
            created_ms: now,
            modified_ms: now,
            authorized_ms: now,
        };
        let out = summary(&c, "CREATING");
        self.connections.insert(key(r.account(), &[name]), c);
        Ok(out)
    }

    pub(crate) fn update_connection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.connection(r, r.s("Name"))?.name.clone();
        let now = self.now_ms();
        let Some(c) = self.connections.get_mut(&key(r.account(), &[&name])) else {
            return Err(not_found(format!("Connection '{name}' does not exist.")));
        };
        if let Some(d) = r.opt("Description") {
            c.description = Some(d.to_owned());
        }
        if let Some(t) = r.opt("AuthorizationType") {
            t.clone_into(&mut c.auth_type);
        }
        if let Some(new) = r.v.get("AuthParameters") {
            let mut old: Value = serde_json::from_str(&c.auth_parameters).unwrap_or(Value::Null);
            merge(&mut old, new);
            c.auth_parameters = old.to_string();
        }
        if let Some(p) = r.json_text("InvocationConnectivityParameters") {
            c.connectivity = Some(p);
        }
        c.modified_ms = now;
        c.state = "AUTHORIZED".to_owned();
        Ok(summary(c, "UPDATING"))
    }

    pub(crate) fn deauthorize_connection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.connection(r, r.s("Name"))?.name.clone();
        let now = self.now_ms();
        let Some(c) = self.connections.get_mut(&key(r.account(), &[&name])) else {
            return Err(not_found(format!("Connection '{name}' does not exist.")));
        };
        "DEAUTHORIZED".clone_into(&mut c.state);
        c.modified_ms = now;
        Ok(summary(c, "DEAUTHORIZING"))
    }

    pub(crate) fn delete_connection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.connection(r, r.s("Name"))?.name.clone();
        let Some(c) = self.connections.remove(&key(r.account(), &[&name])) else {
            return Err(not_found(format!("Connection '{name}' does not exist.")));
        };
        Ok(summary(&c, "DELETING"))
    }

    pub(crate) fn describe_connection(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let c = self.connection(r, r.s("Name"))?;
        let params: Value = serde_json::from_str(&c.auth_parameters).unwrap_or(Value::Null);
        Ok(Out::new()
            .with("ConnectionArn", c.arn.clone())
            .with("Name", c.name.clone())
            .maybe("Description", c.description.clone())
            .json("InvocationConnectivityParameters", c.connectivity.as_ref())
            .with("ConnectionState", c.state.clone())
            .with("AuthorizationType", c.auth_type.clone())
            .with("SecretArn", c.secret_arn.clone())
            .maybe("KmsKeyIdentifier", c.kms_key.clone())
            .with("AuthParameters", redact(&params))
            .with("CreationTime", secs(c.created_ms))
            .with("LastModifiedTime", secs(c.modified_ms))
            .with("LastAuthorizedTime", secs(c.authorized_ms))
            .done())
    }

    pub(crate) fn list_connections(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let state = r.opt("ConnectionState");
        let mine = key(r.account(), &[""]);
        let items = self
            .connections
            .with_prefix(&mine)
            .filter(|(_, c)| c.name.starts_with(prefix))
            .filter(|(_, c)| state.is_none_or(|s| c.state == s))
            .map(|(k, c)| (k.to_owned(), c));
        let (conns, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = conns
            .into_iter()
            .map(|c| {
                Out::new()
                    .with("ConnectionArn", c.arn.clone())
                    .with("Name", c.name.clone())
                    .with("ConnectionState", c.state.clone())
                    .with("AuthorizationType", c.auth_type.clone())
                    .with("CreationTime", secs(c.created_ms))
                    .with("LastModifiedTime", secs(c.modified_ms))
                    .with("LastAuthorizedTime", secs(c.authorized_ms))
                    .done()
            })
            .collect();
        Ok(with_token(
            Out::new().with("Connections", list).done(),
            token,
        ))
    }
}

/// What create, update, delete and deauthorize answer: the connection and the state it is moving to.
fn summary(c: &Connection, state: &str) -> Value {
    Out::new()
        .with("ConnectionArn", c.arn.clone())
        .with("ConnectionState", state)
        .with("CreationTime", secs(c.created_ms))
        .with("LastModifiedTime", secs(c.modified_ms))
        .with("LastAuthorizedTime", secs(c.authorized_ms))
        .done()
}

/// `new` laid over `old`, member by member for objects and whole for anything else.
fn merge(old: &mut Value, new: &Value) {
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for (k, v) in n {
                merge(o.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (o, n) => *o = n.clone(),
    }
}

/// `AuthParameters` as `DescribeConnection` shows it: no password, key value, client secret or secret parameter value.
fn redact(params: &Value) -> Value {
    let mut out = Map::new();
    let Some(map) = params.as_object() else {
        return Value::Object(out);
    };
    for (block, v) in map {
        let shown = match block.as_str() {
            "ApiKeyAuthParameters" => without(v, &["ApiKeyValue"]),
            "BasicAuthParameters" => without(v, &["Password"]),
            "OAuthParameters" => oauth(v),
            "InvocationHttpParameters" => http_parameters(v),
            _ => v.clone(),
        };
        out.insert(block.clone(), shown);
    }
    Value::Object(out)
}

fn without(v: &Value, secret_members: &[&str]) -> Value {
    let mut v = v.clone();
    if let Some(m) = v.as_object_mut() {
        for s in secret_members {
            m.remove(*s);
        }
    }
    v
}

fn oauth(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(m) = v.as_object_mut() {
        if let Some(c) = m.get_mut("ClientParameters") {
            *c = without(c, &["ClientSecret"]);
        }
        if let Some(h) = m.get_mut("OAuthHttpParameters") {
            *h = http_parameters(h);
        }
    }
    v
}

/// The header, query string and body parameters, without the values of those marked secret.
fn http_parameters(v: &Value) -> Value {
    let mut v = v.clone();
    for kind in [
        "HeaderParameters",
        "QueryStringParameters",
        "BodyParameters",
    ] {
        let Some(list) = v.get_mut(kind).and_then(Value::as_array_mut) else {
            continue;
        };
        for p in list {
            if p.get("IsValueSecret").and_then(Value::as_bool) == Some(true) {
                *p = without(p, &["Value"]);
            }
        }
    }
    v
}
