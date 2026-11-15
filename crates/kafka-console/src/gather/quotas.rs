//! The quotas of the Access page: what is set, read with DescribeClientQuotas as the console's principal (so it is what any
//! client with the right would be told), and what the broker has metered of each client lately (`Broker::client_quotas`,
//! which the protocol does not say). A quota is set or removed with AlterClientQuotas, as `kafka-configs.sh` does.

use kafka_broker::{ClientQuotas, QuotaKind};
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::alter_client_quotas_request::{EntityData, EntryData, OpData};
use kafka_wire::messages::describe_client_quotas_response::EntryData as Described;
use kafka_wire::messages::{AlterClientQuotasRequest, DescribeClientQuotasRequest};

use crate::backend::Backend;
use crate::model::{AccessQuota, AccessQuotaClient, AccessQuotaPart, AccessQuotas, Refusal};
use crate::quota_words::{applies, limit, rate, value_text};
use crate::refusal::refusal;

/// The most quotas and metered clients the page lists.
pub const MOST_ROWS: usize = 500;

/// The quotas set and the clients metered, or DescribeClientQuotas's refusal.
pub async fn read(b: &Backend) -> AccessQuotas {
    let rows = match b
        .call(Request::DescribeClientQuotas(
            DescribeClientQuotasRequest::default(),
        ))
        .await
    {
        Ok(Response::DescribeClientQuotas(r)) if r.error_code == 0 => Ok(r
            .entries
            .iter()
            .flatten()
            .flat_map(rows_of)
            .take(MOST_ROWS)
            .collect()),
        Ok(Response::DescribeClientQuotas(r)) => {
            Err(refusal(r.error_code, r.error_message.as_deref()))
        }
        _ => Err(refusal(-1, Some("The broker did not answer."))),
    };
    let clients = b
        .client_quotas()
        .map(|q| clients_of(&q))
        .unwrap_or_default();
    match rows {
        Ok(rows) => AccessQuotas {
            rows,
            clients,
            refused: None,
        },
        Err(r) => AccessQuotas {
            rows: vec![],
            clients,
            refused: Some(r),
        },
    }
}

/// One row per key of a described entity.
fn rows_of(e: &Described) -> Vec<AccessQuota> {
    let mut parts: Vec<AccessQuotaPart> = (e.entity.iter())
        .map(|p| AccessQuotaPart {
            kind: p.entity_type.to_string(),
            name: p.entity_name.as_ref().map(ToString::to_string),
        })
        .collect();
    parts.sort_by(|a, b| a.kind.cmp(&b.kind));
    let part = |kind: &str| {
        parts
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.name.as_deref())
    };
    let who = applies(part("user"), part("client-id"), part("ip"));
    let enforced = part("ip").is_none();
    (e.values.iter())
        .map(|v| AccessQuota {
            parts: parts.clone(),
            key: v.key.to_string(),
            value: value_text(v.value),
            limit: limit(&v.key, v.value),
            applies: who.clone(),
            enforced,
        })
        .collect()
}

/// The metered clients, the muted first, then by quota, user and client id.
#[must_use]
pub fn clients_of(q: &ClientQuotas) -> Vec<AccessQuotaClient> {
    // A meter with no limit is one Kafka keeps for every client once any quota of its kind is set: not one a quota holds.
    let mut out: Vec<AccessQuotaClient> = (q.usage.iter())
        .filter(|u| u.limit.is_some())
        .map(|u| {
            let key = u.kind.key();
            AccessQuotaClient {
                key: key.to_owned(),
                user: u.user.clone(),
                client_id: u.client_id.clone(),
                rate: rate(key, u.rate),
                limit: u.limit.map(|l| limit(key, l)),
                share: u.limit.filter(|l| *l > 0.0).map(|l| share(u.rate, l)),
                muted_ms: (u.throttled_until_ms - q.now_ms).max(0),
                last_mute_ms: u.throttled_for_ms,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (b.muted_ms > 0, &a.key, &a.user, &a.client_id).cmp(&(
            a.muted_ms > 0,
            &b.key,
            &b.user,
            &b.client_id,
        ))
    });
    out.truncate(MOST_ROWS);
    out
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a percentage, rounded and held to 0..=9999"
)]
fn share(rate: f64, limit: f64) -> u32 {
    (rate / limit * 100.0).round().clamp(0.0, 9999.0) as u32
}

/// What the Set quota dialog posts: the user and the client id (absent: not part of the entity; empty: the default), the
/// quota, and its value (absent: remove it).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct QuotaPost {
    pub user: Option<String>,
    pub client: Option<String>,
    pub key: String,
    pub value: Option<f64>,
}

/// The one quota as AlterClientQuotas carries it.
#[must_use]
pub fn alter_request(p: &QuotaPost) -> AlterClientQuotasRequest {
    let mut entry = EntryData::default();
    for (kind, name) in [("user", &p.user), ("client-id", &p.client)] {
        if let Some(name) = name {
            let mut part = EntityData::default();
            part.entity_type = StrBytes::from_static_str(kind);
            part.entity_name = (!name.is_empty()).then(|| StrBytes::from_string(name.clone()));
            entry.entity.push(part);
        }
    }
    let mut op = OpData::default();
    op.key = StrBytes::from_string(p.key.clone());
    op.value = p.value.unwrap_or(0.0);
    op.remove = p.value.is_none();
    entry.ops.push(op);
    let mut req = AlterClientQuotasRequest::default();
    req.entries.push(entry);
    req
}

/// Sets or removes the one quota, as the console.
///
/// # Errors
/// Kafka's refusal, in its words.
pub async fn alter(b: &Backend, p: &QuotaPost) -> Result<(), Refusal> {
    match b.call(Request::AlterClientQuotas(alter_request(p))).await {
        Ok(Response::AlterClientQuotas(r)) => match r.entries.first() {
            Some(e) if e.error_code != 0 => Err(refusal(e.error_code, e.error_message.as_deref())),
            Some(_) => Ok(()),
            None => Err(refusal(-1, Some("The broker answered for no entity."))),
        },
        _ => Err(refusal(-1, Some("The broker did not answer."))),
    }
}

/// The quotas the dialog offers, in Kafka's order.
#[must_use]
pub fn keys() -> Vec<&'static str> {
    QuotaKind::ALL.iter().map(|k| k.key()).collect()
}

#[cfg(test)]
mod tests {
    use kafka_broker::QuotaUsage;

    use super::*;

    #[test]
    fn the_request_names_only_the_parts_given_and_an_empty_name_is_the_default() {
        let req = alter_request(&QuotaPost {
            user: Some(String::new()),
            client: Some("c1".into()),
            key: "producer_byte_rate".into(),
            value: Some(1024.0),
        });
        let e = &req.entries[0];
        let parts: Vec<(String, Option<String>)> = (e.entity.iter())
            .map(|p| {
                (
                    p.entity_type.to_string(),
                    p.entity_name.as_ref().map(ToString::to_string),
                )
            })
            .collect();
        assert_eq!(
            parts,
            [
                ("user".into(), None),
                ("client-id".into(), Some("c1".into()))
            ]
        );
        assert!(!e.ops[0].remove);
        let removal = alter_request(&QuotaPost {
            user: Some("alice".into()),
            client: None,
            key: "consumer_byte_rate".into(),
            value: None,
        });
        assert!(removal.entries[0].ops[0].remove && removal.entries[0].entity.len() == 1);
    }

    #[test]
    fn a_muted_client_comes_first_with_the_time_it_has_left() {
        let usage = |client: &str, until: i64| QuotaUsage {
            kind: QuotaKind::Produce,
            user: String::new(),
            client_id: client.into(),
            rate: 2048.0,
            limit: Some(1024.0),
            throttled_until_ms: until,
            throttled_for_ms: 900,
            last_record_ms: 1000,
        };
        let q = ClientQuotas {
            set: vec![],
            usage: vec![usage("a", 0), usage("b", 1500)],
            now_ms: 1000,
        };
        let rows = clients_of(&q);
        assert_eq!(rows[0].client_id, "b");
        assert_eq!((rows[0].muted_ms, rows[1].muted_ms), (500, 0));
        assert_eq!(rows[0].share, Some(200));
        assert_eq!(rows[0].rate, "2.0 KiB/s");
    }
}
