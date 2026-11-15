//! What the console is told of the quotas: what is set, and what clients have used of it lately.

use kafka_auth::authz::Operation;
use kafka_meta::QuotaEntity;
use kafka_storage::fs::Fs;

use super::Usage;
use crate::{Broker, Caller, clock};

/// The quotas set and what each client has used lately.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientQuotas {
    /// Every entity with a quota and its values, by entity.
    pub set: Vec<(QuotaEntity, Vec<(String, f64)>)>,
    /// What each metered client has used in the last hour, by quota, user and client id.
    pub usage: Vec<Usage>,
    /// When the usage was measured, in milliseconds since the epoch: a client is muted now if its `throttled_until_ms` is
    /// after it.
    pub now_ms: i64,
}

impl<F: Fs> Broker<F> {
    /// The quotas set and what each client has used of them lately, if `caller` may DESCRIBE_CONFIGS the cluster (what
    /// DescribeClientQuotas asks). Reading it measures the clients' rates as of now; it records nothing.
    #[must_use]
    pub fn client_quotas(&self, caller: &Caller) -> Option<ClientQuotas> {
        if !self.cluster_allows(caller, Operation::DescribeConfigs) {
            return None;
        }
        let now_ms = clock::now().get();
        Some(ClientQuotas {
            set: (self.quotas.set().into_iter())
                .map(|(e, v)| (e, v.into_iter().collect()))
                .collect(),
            usage: self.quotas.usage(now_ms),
            now_ms,
        })
    }
}
