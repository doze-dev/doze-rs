//! Event buses. Every account has the default bus; the others are made, described, updated and deleted. Deleting a bus that has rules is
//! refused (documented: "All rules associated with this event bus need to be deleted first").

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::{Bus, key};
use crate::names::{DEFAULT_BUS, bus_arn, bus_name};
use crate::page::{page, with_token};
use crate::req::{Req, exists, invalid, no_bus};
use crate::util::{Out, secs};

impl Core {
    /// Makes the account's default bus the first time the account is seen.
    pub(crate) fn ensure_default(&mut self, account: &str, region: &str) {
        let k = key(account, &[DEFAULT_BUS]);
        if self.buses.get(&k).is_some() {
            return;
        }
        let now = self.now_ms();
        self.buses.insert(
            k,
            Bus {
                account: account.to_owned(),
                name: DEFAULT_BUS.to_owned(),
                arn: bus_arn(region, account, DEFAULT_BUS),
                description: None,
                kms_key: None,
                dead_letter: None,
                log_config: None,
                policy: None,
                tags: Default::default(),
                created_ms: now,
                modified_ms: now,
            },
        );
    }

    /// The bus a request names (a name or an ARN; the default bus when it names none).
    ///
    /// # Errors
    /// `ResourceNotFoundException` when there is no such bus.
    pub(crate) fn bus<'a>(
        &'a self,
        account: &str,
        name_or_arn: Option<&str>,
    ) -> Result<&'a Bus, ApiError> {
        let given = name_or_arn.filter(|n| !n.is_empty()).unwrap_or(DEFAULT_BUS);
        let name =
            bus_name(given).ok_or_else(|| invalid("Parameter EventBusName is not valid."))?;
        self.buses
            .get(&key(account, &[name]))
            .ok_or_else(|| no_bus(name))
    }

    pub(crate) fn create_bus(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        let k = key(r.account(), &[name]);
        if self.buses.get(&k).is_some() {
            return Err(exists(format!("Event bus {name} already exists.")));
        }
        let now = self.now_ms();
        let arn = bus_arn(r.region(), r.account(), name);
        let bus = Bus {
            account: r.account().to_owned(),
            name: name.to_owned(),
            arn: arn.clone(),
            description: r.opt("Description").map(str::to_owned),
            kms_key: r.opt("KmsKeyIdentifier").map(str::to_owned),
            dead_letter: r.json_text("DeadLetterConfig"),
            log_config: r.json_text("LogConfig"),
            policy: None,
            tags: r.tags().into_iter().collect(),
            created_ms: now,
            modified_ms: now,
        };
        let out = Out::new()
            .with("EventBusArn", arn)
            .maybe("Description", bus.description.clone())
            .maybe("KmsKeyIdentifier", bus.kms_key.clone())
            .json("DeadLetterConfig", bus.dead_letter.as_ref())
            .json("LogConfig", bus.log_config.as_ref())
            .done();
        self.buses.insert(k, bus);
        Ok(out)
    }

    pub(crate) fn delete_bus(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = bus_name(r.s("Name")).ok_or_else(|| invalid("Parameter Name is not valid."))?;
        if name == DEFAULT_BUS {
            // (memory) the wording; the refusal is documented ("You can't delete your account's default event bus").
            return Err(invalid("Cannot delete event bus default."));
        }
        // A bus that is not there is deleted all the same (recorded 2026-11-09).
        let prefix = key(r.account(), &[name, ""]);
        if self.rules.with_prefix(&prefix).next().is_some() {
            return Err(invalid(format!(
                "Event bus {name} has rules associated with it. Delete the rules first."
            )));
        }
        self.buses.remove(&key(r.account(), &[name]));
        Ok(Value::Null)
    }

    pub(crate) fn describe_bus(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let b = self.bus(r.account(), r.opt("Name"))?;
        Ok(Out::new()
            .with("Name", b.name.clone())
            .with("Arn", b.arn.clone())
            .maybe("Description", b.description.clone())
            .maybe("KmsKeyIdentifier", b.kms_key.clone())
            .json("DeadLetterConfig", b.dead_letter.as_ref())
            .maybe("Policy", b.policy.clone())
            .json("LogConfig", b.log_config.as_ref())
            .with("CreationTime", secs(b.created_ms))
            .with("LastModifiedTime", secs(b.modified_ms))
            .done())
    }

    pub(crate) fn list_buses(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("NamePrefix").unwrap_or_default();
        let mine = key(r.account(), &[""]);
        let items = self
            .buses
            .with_prefix(&mine)
            .filter(|(_, b)| b.name.starts_with(prefix))
            .map(|(k, b)| (k.to_owned(), b));
        let (buses, token) = page(items, r, "Limit")?;
        let list: Vec<Value> = buses
            .into_iter()
            .map(|b| {
                Out::new()
                    .with("Name", b.name.clone())
                    .with("Arn", b.arn.clone())
                    .maybe("Description", b.description.clone())
                    .maybe("Policy", b.policy.clone())
                    .with("CreationTime", secs(b.created_ms))
                    .with("LastModifiedTime", secs(b.modified_ms))
                    .done()
            })
            .collect();
        Ok(with_token(
            Out::new().with("EventBuses", list).done(),
            token,
        ))
    }

    pub(crate) fn update_bus(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = self.bus(r.account(), r.opt("Name"))?.name.clone();
        let now = self.now_ms();
        let k = key(r.account(), &[&name]);
        let b = self.buses.get_mut(&k).ok_or_else(|| no_bus(&name))?;
        if let Some(d) = r.opt("Description") {
            b.description = Some(d.to_owned());
        }
        if let Some(k) = r.opt("KmsKeyIdentifier") {
            b.kms_key = Some(k.to_owned());
        }
        if let Some(d) = r.json_text("DeadLetterConfig") {
            b.dead_letter = Some(d);
        }
        if let Some(l) = r.json_text("LogConfig") {
            b.log_config = Some(l);
        }
        b.modified_ms = now;
        Ok(Out::new()
            .with("Arn", b.arn.clone())
            .with("Name", b.name.clone())
            .maybe("KmsKeyIdentifier", b.kms_key.clone())
            .maybe("Description", b.description.clone())
            .json("DeadLetterConfig", b.dead_letter.as_ref())
            .json("LogConfig", b.log_config.as_ref())
            .done())
    }
}
