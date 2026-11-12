//! API destinations: an HTTP endpoint, a method and a connection. They are stored and described. **Nothing calls the endpoint**: a rule
//! target that is an API destination is not delivered (the log says so), because EventBridge's HTTP client, its OAuth token fetch and
//! its body-parameter merging are deferred (SCOPE.md). A destination is `ACTIVE` while its connection exists and is authorized.
//!
//! The wording of refusals is from memory.

use aws_wire::ApiError;
use aws_wire::RequestId;
use serde_json::Value;

use crate::core::Core;
use crate::model::{ApiDestination, key};
use crate::names::{Parts, with_uuid};
use crate::page::{page, with_token};
use crate::req::{Req, exists, invalid, not_found};
use crate::util::{Out, secs};

/// The rate AWS gives a destination that names none.
const DEFAULT_RATE: i64 = 300;

impl Core {
    fn destination(&self, r: &Req<'_>, name: &str) -> Result<&ApiDestination, ApiError> {
        self.destinations
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| not_found(format!("An api-destination '{name}' does not exist.")))
    }

    /// Whether the connection exists and is authorized.
    fn is_active(&self, d: &ApiDestination) -> bool {
        Parts::of(&d.connection_arn)
            .and_then(|p| p.tail.split('/').next())
            .and_then(|n| self.connections.get(&key(&d.account, &[n])))
            .is_some_and(|c| c.arn == d.connection_arn && c.state == "AUTHORIZED")
    }

    fn check_connection(&self, r: &Req<'_>, arn: &str) -> Result<(), ApiError> {
        let name = Parts::of(arn)
            .filter(|p| p.kind == "connection")
            .and_then(|p| p.tail.split('/').next())
            .ok_or_else(|| invalid("Parameter ConnectionArn is not valid."))?;
        self.connection_named(r, name).map(drop)
    }

    fn connection_named(
        &self,
        r: &Req<'_>,
        name: &str,
    ) -> Result<&crate::model::Connection, ApiError> {
        self.connections
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| not_found(format!("Connection '{name}' does not exist.")))
    }

    pub(crate) fn create_destination(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        if self.destinations.get(&key(r.account(), &[name])).is_some() {
            return Err(exists(format!(
                "An api-destination '{name}' already exists."
            )));
        }
        self.check_connection(r, r.s("ConnectionArn"))?;
        let now = self.now_ms();
        let d = ApiDestination {
            account: r.account().to_owned(),
            name: name.to_owned(),
            arn: with_uuid(
                r.region(),
                r.account(),
                "api-destination",
                name,
                &RequestId::new().to_string(),
            ),
            description: r.opt("Description").map(str::to_owned),
            connection_arn: r.s("ConnectionArn").to_owned(),
            endpoint: r.s("InvocationEndpoint").to_owned(),
            method: r.s("HttpMethod").to_owned(),
            rate_limit: Some(
                r.int("InvocationRateLimitPerSecond")
                    .unwrap_or(DEFAULT_RATE),
            ),
            created_ms: now,
            modified_ms: now,
        };
        let out = self.summary(&d);
        self.destinations.insert(key(r.account(), &[name]), d);
        Ok(out)
    }

    pub(crate) fn update_destination(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.destination(r, r.s("Name"))?.name.clone();
        if let Some(arn) = r.opt("ConnectionArn") {
            self.check_connection(r, arn)?;
        }
        let now = self.now_ms();
        let Some(d) = self.destinations.get_mut(&key(r.account(), &[&name])) else {
            return Err(not_found(format!(
                "An api-destination '{name}' does not exist."
            )));
        };
        if let Some(v) = r.opt("Description") {
            d.description = Some(v.to_owned());
        }
        if let Some(v) = r.opt("ConnectionArn") {
            v.clone_into(&mut d.connection_arn);
        }
        if let Some(v) = r.opt("InvocationEndpoint") {
            v.clone_into(&mut d.endpoint);
        }
        if let Some(v) = r.opt("HttpMethod") {
            v.clone_into(&mut d.method);
        }
        if let Some(v) = r.int("InvocationRateLimitPerSecond") {
            d.rate_limit = Some(v);
        }
        d.modified_ms = now;
        let d = d.clone();
        Ok(self.summary(&d))
    }

    pub(crate) fn delete_destination(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.destination(r, r.s("Name"))?.name.clone();
        self.destinations.remove(&key(r.account(), &[&name]));
        Ok(Value::Null)
    }

    pub(crate) fn describe_destination(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let d = self.destination(r, r.s("Name"))?;
        Ok(Out::new()
            .with("ApiDestinationArn", d.arn.clone())
            .with("Name", d.name.clone())
            .maybe("Description", d.description.clone())
            .with("ApiDestinationState", self.state(d))
            .with("ConnectionArn", d.connection_arn.clone())
            .with("InvocationEndpoint", d.endpoint.clone())
            .with("HttpMethod", d.method.clone())
            .maybe("InvocationRateLimitPerSecond", d.rate_limit)
            .with("CreationTime", secs(d.created_ms))
            .with("LastModifiedTime", secs(d.modified_ms))
            .done())
    }

    pub(crate) fn list_destinations(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let connection = r.opt("ConnectionArn");
        let mine = key(r.account(), &[""]);
        let items = self
            .destinations
            .with_prefix(&mine)
            .filter(|(_, d)| d.name.starts_with(prefix))
            .filter(|(_, d)| connection.is_none_or(|c| d.connection_arn == c))
            .map(|(k, d)| (k.to_owned(), d));
        let (found, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = found
            .into_iter()
            .map(|d| {
                Out::new()
                    .with("ApiDestinationArn", d.arn.clone())
                    .with("Name", d.name.clone())
                    .with("ApiDestinationState", self.state(d))
                    .with("ConnectionArn", d.connection_arn.clone())
                    .with("InvocationEndpoint", d.endpoint.clone())
                    .with("HttpMethod", d.method.clone())
                    .maybe("InvocationRateLimitPerSecond", d.rate_limit)
                    .with("CreationTime", secs(d.created_ms))
                    .with("LastModifiedTime", secs(d.modified_ms))
                    .done()
            })
            .collect();
        Ok(with_token(
            Out::new().with("ApiDestinations", list).done(),
            token,
        ))
    }

    fn state(&self, d: &ApiDestination) -> &'static str {
        if self.is_active(d) {
            "ACTIVE"
        } else {
            "INACTIVE"
        }
    }

    /// What create and update answer.
    fn summary(&self, d: &ApiDestination) -> Value {
        Out::new()
            .with("ApiDestinationArn", d.arn.clone())
            .with("ApiDestinationState", self.state(d))
            .with("CreationTime", secs(d.created_ms))
            .with("LastModifiedTime", secs(d.modified_ms))
            .done()
    }
}
