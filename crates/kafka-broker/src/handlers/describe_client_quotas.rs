//! DescribeClientQuotas (API 48): the quotas set for the entities a filter matches.

use kafka_auth::authz::Operation;
use kafka_storage::fs::Fs;
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::describe_client_quotas_response::{EntityData, EntryData, ValueData};
use kafka_wire::messages::{DescribeClientQuotasRequest, DescribeClientQuotasResponse};

use crate::quotas::entity::{self, Component};
use crate::{Broker, Caller};

impl<F: Fs> Broker<F> {
    /// The entities the filter matches and every value set for each (`ClientQuotasImage.describe`): the filter's rules and
    /// words are in [`entity::describe`]. A filter that is refused is answered with its error and message and no entries
    /// (`DescribeClientQuotasRequest.getErrorResponse`). The caller needs DESCRIBE_CONFIGS on the cluster; without it the
    /// answer is CLUSTER_AUTHORIZATION_FAILED, Kafka's message for it and no entries.
    ///
    /// Each entry lists its entity's parts client id first, then user, as Kafka's hash map happens to; the entries and their
    /// values are in order of entity and of key, where Kafka's are in its maps' order.
    pub(crate) fn describe_client_quotas(
        &self,
        caller: &Caller,
        req: &DescribeClientQuotasRequest,
    ) -> DescribeClientQuotasResponse {
        let mut resp = DescribeClientQuotasResponse::default();
        if !self.cluster_allows(caller, Operation::DescribeConfigs) {
            return refused(
                ResponseError::ClusterAuthorizationFailed,
                "Cluster authorization failed.",
            );
        }
        let components: Vec<Component> = (req.components.iter())
            .map(|c| Component {
                entity_type: c.entity_type.to_string(),
                match_type: c.match_type,
                name: c._match.as_ref().map(ToString::to_string),
            })
            .collect();
        let set = self.quotas.set();
        let found = match entity::describe(&set, &components, req.strict) {
            Ok(found) => found,
            Err((code, message)) => return refused(code, &message),
        };
        resp.entries = Some(
            found
                .into_iter()
                .map(|(e, values)| {
                    let mut entry = EntryData::default();
                    entry.entity = (entity::named_of(e).into_iter())
                        .map(|(kind, name)| {
                            let mut part = EntityData::default();
                            part.entity_type = StrBytes::from_string(kind);
                            part.entity_name = name.map(StrBytes::from_string);
                            part
                        })
                        .collect();
                    entry.values = (values.iter())
                        .map(|(key, value)| {
                            let mut v = ValueData::default();
                            v.key = StrBytes::from_string(key.clone());
                            v.value = *value;
                            v
                        })
                        .collect();
                    entry
                })
                .collect(),
        );
        resp
    }
}

fn refused(code: ResponseError, message: &str) -> DescribeClientQuotasResponse {
    let mut resp = DescribeClientQuotasResponse::default();
    resp.error_code = code.code();
    resp.error_message = Some(StrBytes::from_string(message.to_owned()));
    resp.entries = None;
    resp
}
