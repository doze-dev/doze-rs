//! Aliases: one or two versions with weights that add up to 100. An execution started through an alias picks a version by weight
//! and records both ARNs.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn::{self, Target};
use crate::core::Core;
use crate::model::{Alias, Machine};
use crate::req::{Req, conflict, invalid, invalid_arn, not_found, put, secs, token, valid_name};

/// The version an alias sends an execution to: a weighted pick (`seed` varies it).
pub(crate) fn pick(routes: &[(u64, u32)], seed: i64) -> u64 {
    let total: u32 = routes.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return routes.first().map_or(0, |(v, _)| *v);
    }
    let mut roll = u32::try_from(seed.rem_euclid(i64::from(total))).unwrap_or(0);
    let mut x = crate::util::uuid()
        .bytes()
        .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    x %= total;
    roll = (roll + x) % total;
    for (v, w) in routes {
        if roll < *w {
            return *v;
        }
        roll -= w;
    }
    routes.last().map_or(0, |(v, _)| *v)
}

/// The machine an alias ARN belongs to, and the alias's name.
fn alias_of(given: &str) -> Result<(String, String), ApiError> {
    match arn::target(given) {
        Some((p, Target::Alias(m, a))) => Ok((arn::machine(p.region, p.account, &m), a)),
        _ => Err(invalid_arn("Arn", given)),
    }
}

/// Reads `routingConfiguration` against the machine's versions.
fn routes(m: &Machine, r: &Req<'_>) -> Result<Vec<(u64, u32)>, ApiError> {
    let list =
        r.v.get("routingConfiguration")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
    let mut out = Vec::new();
    for item in &list {
        let v = item
            .get("stateMachineVersionArn")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let w = item.get("weight").and_then(Value::as_u64).unwrap_or(0);
        let Some((_, Target::Version(name, n))) = arn::target(v) else {
            return Err(invalid_arn("Arn", v));
        };
        if name != m.name || !m.versions.iter().any(|x| x.number == n) {
            return Err(not_found(format!("Resource not found: '{v}'"))); // (memory)
        }
        out.push((n, u32::try_from(w).unwrap_or(0)));
    }
    let total: u32 = out.iter().map(|(_, w)| *w).sum();
    if out.is_empty() || out.len() > 2 || total != 100 {
        // (memory)
        return Err(invalid(
            "The sum of the weights in the routing configuration must be equal to 100",
        ));
    }
    Ok(out)
}

fn routing(m: &Machine, a: &Alias) -> Value {
    Value::Array(
        a.routes
            .iter()
            .map(|(v, w)| json!({"stateMachineVersionArn": format!("{}:{v}", m.arn), "weight": w}))
            .collect(),
    )
}

impl Core {
    pub(crate) fn create_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("name");
        if !valid_name(name) || name.parse::<u64>().is_ok() {
            return Err(crate::req::invalid_name(name));
        }
        let first =
            r.v.pointer("/routingConfiguration/0/stateMachineVersionArn")
                .and_then(Value::as_str)
                .unwrap_or_default();
        let key = arn::target(first)
            .map(|(p, t)| arn::machine(p.region, p.account, t.machine()))
            .ok_or_else(|| invalid_arn("Arn", first))?;
        let now = self.now_ms();
        let m = self
            .machines
            .get(&key)
            .ok_or_else(|| crate::req::no_machine(&key))?;
        let rs = routes(m, r)?;
        let arn = format!("{key}:{name}");
        if let Some(a) = m.aliases.iter().find(|a| a.name == name) {
            if a.routes == rs && a.description.as_deref() == r.opt("description") {
                return Ok(
                    json!({"stateMachineAliasArn": arn, "creationDate": secs(a.created_ms)}),
                );
            }
            return Err(conflict(format!("Alias already exists: '{arn}'"))); // (memory)
        }
        let Some(m) = self.machines.get_mut(&key) else {
            return Err(crate::req::no_machine(&key));
        };
        m.aliases.push(Alias {
            name: name.to_owned(),
            description: r.opt("description").map(str::to_owned),
            routes: rs,
            created_ms: now,
            updated_ms: now,
        });
        Ok(json!({"stateMachineAliasArn": arn, "creationDate": secs(now)}))
    }

    pub(crate) fn describe_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineAliasArn");
        let (key, name) = alias_of(given)?;
        let missing = || not_found(format!("Resource not found: '{given}'"));
        let m = self.machines.get(&key).ok_or_else(missing)?;
        let a = m
            .aliases
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(missing)?;
        let mut out = Map::new();
        out.insert("stateMachineAliasArn".into(), json!(given));
        out.insert("name".into(), json!(a.name));
        put(
            &mut out,
            "description",
            a.description.clone().map(Value::String),
        );
        out.insert("routingConfiguration".into(), routing(m, a));
        out.insert("creationDate".into(), secs(a.created_ms));
        out.insert("updateDate".into(), secs(a.updated_ms));
        Ok(Value::Object(out))
    }

    pub(crate) fn update_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineAliasArn");
        let (key, name) = alias_of(given)?;
        let missing = || not_found(format!("Resource not found: '{given}'"));
        let m = self.machines.get(&key).ok_or_else(missing)?;
        let rs = if r.v.get("routingConfiguration").is_some() {
            Some(routes(m, r)?)
        } else {
            None
        };
        let now = self.now_ms();
        let m = self.machines.get_mut(&key).ok_or_else(missing)?;
        let a = m
            .aliases
            .iter_mut()
            .find(|a| a.name == name)
            .ok_or_else(missing)?;
        if let Some(rs) = rs {
            a.routes = rs;
        }
        if let Some(d) = r.opt("description") {
            a.description = Some(d.to_owned());
        }
        a.updated_ms = now;
        Ok(json!({"updateDate": secs(now)}))
    }

    pub(crate) fn delete_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineAliasArn");
        let (key, name) = alias_of(given)?;
        if let Some(m) = self.machines.get_mut(&key) {
            m.aliases.retain(|a| a.name != name);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_aliases(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let given = r.s("stateMachineArn");
        let m = self
            .machines
            .get(given)
            .ok_or_else(|| crate::req::no_machine(given))?;
        let all: Vec<Value> = m
            .aliases
            .iter()
            .map(|a| json!({"stateMachineAliasArn": format!("{}:{}", m.arn, a.name), "creationDate": secs(a.created_ms)}))
            .collect();
        let (from, size) = (r.page_from()?, r.page_size(100));
        let mut out = json!({"stateMachineAliases": all.iter().skip(from).take(size).cloned().collect::<Vec<_>>()});
        if from + size < all.len() {
            out["nextToken"] = json!(token(from + size));
        }
        Ok(out)
    }
}
