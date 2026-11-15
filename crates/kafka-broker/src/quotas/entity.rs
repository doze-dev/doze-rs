//! The rules of DescribeClientQuotas and AlterClientQuotas, apart from the wire and the store: which entities a filter
//! matches (`ClientQuotasImage.describe`) and what an alteration changes or why it is refused
//! (`ClientQuotaControlManager.alterClientQuotas`). Every message is Kafka's, word for word.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::{IpAddr, ToSocketAddrs};

use kafka_meta::{QuotaEntity, QuotaName};
use kafka_wire::ResponseError;

pub(crate) const USER: &str = "user";
pub(crate) const CLIENT_ID: &str = "client-id";
pub(crate) const IP: &str = "ip";

/// `connection_creation_rate`, the one quota an address can have.
pub(crate) const CONNECTION_RATE: &str = "connection_creation_rate";

/// An entity as a request names it: entity type to name (`None`, the default). A type named twice keeps the last name, as
/// the Java map a request is read into does. Ordered by type, which for the types Kafka knows is also the order its hash map
/// lists them in (`client-id` before `user`).
pub(crate) type Named = BTreeMap<String, Option<String>>;

/// `ClientQuotaEntity.toString`: `ClientQuotaEntity(entries={client-id=c1, user=null})`.
pub(crate) fn describe_named(named: &Named) -> String {
    let mut out = String::from("ClientQuotaEntity(entries={");
    for (i, (kind, name)) in named.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        let _ = write!(out, "{sep}{kind}={}", name.as_deref().unwrap_or("null"));
    }
    out.push_str("})");
    out
}

/// The parts of a stored entity, as a request would name them.
pub(crate) fn named_of(entity: &QuotaEntity) -> Named {
    let mut out = Named::new();
    for (kind, part) in [
        (USER, &entity.user),
        (CLIENT_ID, &entity.client_id),
        (IP, &entity.ip),
    ] {
        if let Some(name) = part {
            out.insert(kind.to_owned(), name.as_wire().map(str::to_owned));
        }
    }
    out
}

/// What one entry of AlterClientQuotas asks: its entity, and each key set to a value or removed (`None`).
#[derive(Debug, Clone)]
pub(crate) struct Alteration {
    pub(crate) entity: Named,
    pub(crate) ops: Vec<(String, Option<f64>)>,
}

/// The answer for each entity of an AlterClientQuotas, and the quotas the broker has if it is applied.
#[derive(Debug)]
pub(crate) struct Altered {
    /// One per distinct entity, in the order each first appears; `Err` is INVALID_REQUEST and its message.
    pub(crate) results: Vec<(Named, Result<(), String>)>,
    /// Every entity's values after the change (an entity left with none is gone).
    pub(crate) set: BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    /// Whether anything changed.
    pub(crate) changed: bool,
}

/// Works out an AlterClientQuotas against the quotas there are, as `ClientQuotaControlManager.alterClientQuotas` does:
/// each entry is judged on its own against what was there before the request; an entry that repeats a key, or repeats an
/// entity an earlier entry altered, is refused as a duplicate (and the refusal takes the place of the earlier entry's answer,
/// whose change still stands, as in Kafka).
pub(crate) fn alter(
    current: &BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    alterations: &[Alteration],
) -> Altered {
    let mut set = current.clone();
    let mut results: Vec<(Named, Result<(), String>)> = Vec::new();
    let mut changed = false;
    for alteration in alterations {
        let mut keys: Vec<&str> = Vec::new();
        let duplicate_key = alteration.ops.iter().any(|(key, _)| {
            let seen = keys.contains(&key.as_str());
            keys.push(key);
            seen
        });
        let seen = results.iter().position(|(n, _)| *n == alteration.entity);
        let outcome = if duplicate_key || seen.is_some() {
            Err(format!(
                "Ignoring duplicate entity {}",
                describe_named(&alteration.entity)
            ))
        } else {
            one(current, alteration).map(|(entity, values)| {
                changed |= current.get(&entity) != Some(&values);
                if values.is_empty() {
                    set.remove(&entity);
                } else {
                    set.insert(entity, values);
                }
            })
        };
        match seen {
            Some(at) => {
                if let Some(slot) = results.get_mut(at) {
                    slot.1 = outcome;
                }
            }
            None => results.push((alteration.entity.clone(), outcome)),
        }
    }
    Altered {
        results,
        set,
        changed,
    }
}

/// One entity's alteration: `alterClientQuotaEntity`. The entity's values after it, or why it is refused.
fn one(
    current: &BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    alteration: &Alteration,
) -> Result<(QuotaEntity, BTreeMap<String, f64>), String> {
    let entity = validate_entity(&alteration.entity)?;
    let mut values = current.get(&entity).cloned().unwrap_or_default();
    // Later ops of the same key cannot happen here: a repeated key was refused before.
    for (key, value) in &alteration.ops {
        match value {
            None => {
                values.remove(key);
            }
            Some(v) => {
                check_value(&entity, key, *v)?;
                values.insert(key.clone(), *v);
            }
        }
    }
    Ok((entity, values))
}

/// `validateEntity` and `configKeysForEntityType`: the types known, no empty name, an address alone or a user and client id.
fn validate_entity(named: &Named) -> Result<QuotaEntity, String> {
    if named.is_empty() {
        return Err("Invalid empty client quota entity".to_owned());
    }
    let mut entity = QuotaEntity {
        user: None,
        client_id: None,
        ip: None,
    };
    for (kind, name) in named {
        let slot = match kind.as_str() {
            USER => &mut entity.user,
            CLIENT_ID => &mut entity.client_id,
            IP => &mut entity.ip,
            other => return Err(format!("Unhandled client quota entity type: {other}")),
        };
        if name.as_deref() == Some("") {
            return Err(format!("Empty {kind} not supported"));
        }
        *slot = Some(QuotaName::from_wire(name.as_deref()));
    }
    if let Some(ip) = &entity.ip {
        if entity.user.is_some() || entity.client_id.is_some() {
            // Kafka's words, its missing space included.
            return Err("Invalid quota entity combination, IP entity shouldnot be combined with User or ClientId".to_owned());
        }
        if let QuotaName::Named(address) = ip
            && !resolvable(address)
        {
            return Err(format!("{address} is not a valid IP or resolvable host."));
        }
    }
    Ok(entity)
}

/// `InetAddress.getByName`: an address, or a name that resolves.
fn resolvable(name: &str) -> bool {
    let bare = name
        .strip_prefix('[')
        .and_then(|n| n.strip_suffix(']'))
        .unwrap_or(name);
    bare.parse::<IpAddr>().is_ok()
        || (bare, 0)
            .to_socket_addrs()
            .is_ok_and(|mut a| a.next().is_some())
}

/// The type of a quota's value, as Kafka's config definitions give it.
#[derive(Clone, Copy)]
enum Type {
    Long,
    Int,
    Double,
}

/// `validateQuotaKeyValue`: a key the entity can have, a finite value above zero, and a whole number where the key's type
/// is one.
fn check_value(entity: &QuotaEntity, key: &str, value: f64) -> Result<(), String> {
    let kind = if entity.ip.is_some() {
        (key == CONNECTION_RATE).then_some(Type::Int)
    } else {
        match key {
            "producer_byte_rate" | "consumer_byte_rate" => Some(Type::Long),
            "request_percentage" | "controller_mutation_rate" => Some(Type::Double),
            _ => None,
        }
    };
    let Some(kind) = kind else {
        return Err(format!("Invalid configuration key {key}"));
    };
    if !value.is_finite() {
        return Err(format!("Quota {key} must be a finite number"));
    }
    if value <= 0.0 {
        return Err(format!("Quota {key} must be greater than 0"));
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "Java compares the double with the long the same way"
    )]
    let (most, name) = match kind {
        Type::Double => return Ok(()),
        Type::Long => (i64::MAX as f64, "a LONG"),
        Type::Int => (f64::from(i32::MAX), "an INT"),
    };
    if value > most {
        return Err(format!("Proposed value for {key} is too large for {name}."));
    }
    if (value % 1.0).abs() > 1e-6 {
        return Err(format!("{key} cannot be a fractional value."));
    }
    Ok(())
}

/// One component of a DescribeClientQuotas filter.
#[derive(Debug, Clone)]
pub(crate) struct Component {
    pub(crate) entity_type: String,
    pub(crate) match_type: i8,
    pub(crate) name: Option<String>,
}

const MATCH_EXACT: i8 = 0;
const MATCH_DEFAULT: i8 = 1;
const MATCH_SPECIFIED: i8 = 2;

/// The entities a DescribeClientQuotas filter matches (`ClientQuotasImage.describe`), or why the filter is refused.
pub(crate) fn describe<'a>(
    set: &'a BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    components: &[Component],
    strict: bool,
) -> Result<Vec<(&'a QuotaEntity, &'a BTreeMap<String, f64>)>, (ResponseError, String)> {
    let (exact, typed) = read_filter(components)?;
    let invalid = |m: &str| (ResponseError::InvalidRequest, m.to_owned());
    let named = |t: &str| exact.iter().any(|(k, _)| *k == t) || typed.contains(&t);
    if named(IP) && (named(USER) || named(CLIENT_ID)) {
        return Err(invalid(
            "Invalid entity filter component combination. IP filter component should not be used with user or clientId filter component.",
        ));
    }
    if exact.is_empty() && typed.is_empty() {
        return Ok(if strict {
            Vec::new()
        } else {
            set.iter().collect()
        });
    }
    let wanted = exact.len() + typed.len();
    Ok(set
        .iter()
        .filter(|(entity, _)| {
            let parts = named_of(entity);
            exact
                .iter()
                .all(|(t, n)| parts.get(*t).is_some_and(|have| have.as_deref() == *n))
                && typed.iter().all(|t| parts.contains_key(*t))
                && (!strict || parts.len() == wanted)
        })
        .collect())
}

type Filter<'a> = (Vec<(&'a str, Option<&'a str>)>, Vec<&'a str>);

/// The filter's components, checked in Kafka's order: an empty type, a type twice, a type it does not know, then the match.
fn read_filter(components: &[Component]) -> Result<Filter<'_>, (ResponseError, String)> {
    let invalid = |m: String| (ResponseError::InvalidRequest, m);
    let mut exact: Vec<(&str, Option<&str>)> = Vec::new();
    let mut typed: Vec<&str> = Vec::new();
    for c in components {
        let t = c.entity_type.as_str();
        if t.is_empty() {
            return Err(invalid("Invalid empty entity type.".to_owned()));
        }
        if exact.iter().any(|(k, _)| *k == t) || typed.contains(&t) {
            return Err(invalid(format!(
                "Entity type {t} cannot appear more than once in the filter."
            )));
        }
        if ![IP, USER, CLIENT_ID].contains(&t) {
            return Err((
                ResponseError::UnsupportedVersion,
                format!("Unsupported entity type {t}"),
            ));
        }
        match (c.match_type, c.name.as_deref()) {
            (MATCH_EXACT, None) => {
                return Err(invalid(
                    "Request specified MATCH_TYPE_EXACT, but set match string to null.".to_owned(),
                ));
            }
            (MATCH_EXACT, Some(name)) => exact.push((t, Some(name))),
            (MATCH_DEFAULT, Some(_)) => {
                return Err(invalid(
                    "Request specified MATCH_TYPE_DEFAULT, but also specified a match string."
                        .to_owned(),
                ));
            }
            (MATCH_DEFAULT, None) => exact.push((t, None)),
            (MATCH_SPECIFIED, Some(_)) => {
                return Err(invalid(
                    "Request specified MATCH_TYPE_SPECIFIED, but also specified a match string."
                        .to_owned(),
                ));
            }
            (MATCH_SPECIFIED, None) => typed.push(t),
            (other, _) => return Err(invalid(format!("Unknown match type {other}"))),
        }
    }
    Ok((exact, typed))
}

#[cfg(test)]
mod tests;
