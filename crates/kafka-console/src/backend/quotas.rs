//! What the broker has metered of its clients' quotas, which no Kafka request tells.

use kafka_broker::ClientQuotas;

use super::Backend;

impl Backend {
    /// The quotas set and what each client has used of them lately, if the console may DESCRIBE_CONFIGS the cluster (as
    /// DescribeClientQuotas asks).
    #[must_use]
    pub fn client_quotas(&self) -> Option<ClientQuotas> {
        self.broker.client_quotas(&self.caller)
    }
}
