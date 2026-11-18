//! AlterClientQuotas (API 49): set or remove quotas, durably, and put them in force.

use std::collections::BTreeMap;

use kafka_auth::authz::Operation;
use kafka_meta::{QuotaEntity, QuotaValues};
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::alter_client_quotas_response::{EntityData, EntryData};
use kafka_wire::messages::{AlterClientQuotasRequest, AlterClientQuotasResponse};

use crate::quotas::entity::{self, Alteration, Named};
use crate::{Broker, Caller, blocking};

impl<F: Fs> Broker<F> {
    /// Judges each entry as [`entity::alter`] does (Kafka's rules and words), stores the result in one durable write and
    /// then puts it in force, unless `validate_only`. One answer per entity, in the order each first appears (Kafka's are in
    /// its hash map's order). A store that fails answers every entry KAFKA_STORAGE_ERROR and changes nothing.
    ///
    /// The caller needs ALTER_CONFIGS on the cluster (`ControllerApis.handleAlterClientQuotas`); without it every entry of
    /// the request is CLUSTER_AUTHORIZATION_FAILED with Kafka's message for it, as it was sent
    /// (`AlterClientQuotasRequest.getErrorResponse`).
    pub(crate) async fn alter_client_quotas(
        &self,
        caller: &Caller,
        req: &AlterClientQuotasRequest,
    ) -> AlterClientQuotasResponse {
        if !self.cluster_allows(caller, Operation::AlterConfigs) {
            return all_refused(req, ResponseError::ClusterAuthorizationFailed);
        }
        let alterations: Vec<Alteration> = req.entries.iter().map(alteration_of).collect();
        let _one_change_at_a_time = self.quota_admin.lock().await;
        let current = self.quotas.set();
        // An address entity is checked with a name lookup, as Kafka's `InetAddress.getByName` does.
        let altered = blocking::run(|| entity::alter(&current, &alterations));
        if !req.validate_only && altered.changed {
            let (puts, removals) = changes(&current, &altered.set);
            if let Err(e) = blocking::run(|| self.meta.alter_quotas(&puts, &removals)) {
                tracing::error!(error = %e, "client quotas could not be stored");
                return all_refused(req, ResponseError::KafkaStorageError);
            }
            self.quotas.replace(altered.set);
        }
        let mut resp = AlterClientQuotasResponse::default();
        resp.entries = (altered.results.into_iter())
            .map(|(named, result)| {
                let mut entry = EntryData::default();
                entry.entity = parts(&named);
                match result {
                    Ok(()) => entry.error_message = None,
                    Err(message) => {
                        entry.error_code = ResponseError::InvalidRequest.code();
                        entry.error_message = Some(StrBytes::from_string(message));
                    }
                }
                entry
            })
            .collect();
        resp
    }
}

/// One entry of the request as the rules read it: a type named twice keeps its last name, a removal has no value.
fn alteration_of(e: &kafka_wire::messages::alter_client_quotas_request::EntryData) -> Alteration {
    Alteration {
        entity: (e.entity.iter())
            .map(|p| {
                (
                    p.entity_type.to_string(),
                    p.entity_name.as_ref().map(ToString::to_string),
                )
            })
            .collect(),
        ops: (e.ops.iter())
            .map(|op| (op.key.to_string(), (!op.remove).then_some(op.value)))
            .collect(),
    }
}

/// What to write so that the store holds `after` where it held `before`.
fn changes(
    before: &BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
    after: &BTreeMap<QuotaEntity, BTreeMap<String, f64>>,
) -> (Vec<(QuotaEntity, QuotaValues)>, Vec<QuotaEntity>) {
    let puts = (after.iter())
        .filter(|(e, v)| before.get(*e) != Some(*v))
        .map(|(e, v)| (e.clone(), QuotaValues(v.clone())))
        .collect();
    let removals = (before.keys())
        .filter(|e| !after.contains_key(*e))
        .cloned()
        .collect();
    (puts, removals)
}

fn parts(named: &Named) -> Vec<EntityData> {
    (named.iter())
        .map(|(kind, name)| {
            let mut part = EntityData::default();
            part.entity_type = StrBytes::from_string(kind.clone());
            part.entity_name = name.clone().map(StrBytes::from_string);
            part
        })
        .collect()
}

/// Every entry of the request answered `code` with Kafka's message for it, its entity as it was sent.
fn all_refused(req: &AlterClientQuotasRequest, code: ResponseError) -> AlterClientQuotasResponse {
    let message = match code {
        ResponseError::ClusterAuthorizationFailed => "Cluster authorization failed.",
        _ => "Disk error when trying to access log file on the disk.",
    };
    let mut resp = AlterClientQuotasResponse::default();
    resp.entries = (req.entries.iter())
        .map(|e| {
            let mut entry = EntryData::default();
            entry.error_code = code.code();
            entry.error_message = Some(StrBytes::from_string(message.to_owned()));
            entry.entity = (e.entity.iter())
                .map(|p| {
                    let mut part = EntityData::default();
                    part.entity_type = p.entity_type.clone();
                    part.entity_name = p.entity_name.clone();
                    part
                })
                .collect();
            entry
        })
        .collect();
    resp
}
