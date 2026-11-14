//! What the Settings and Access pages read from the broker that the protocol does not say: the intervals, the names of
//! the flag users and super users, the size of the data directory, and the APIs ApiVersions answers.

use kafka_broker::{AccessFacts, DataSize, Intervals};
use kafka_wire::profile::{Advertised, ApiSet, Features};

use super::Backend;

impl Backend {
    /// The intervals the broker was started with, if the console may describe the broker's configs.
    #[must_use]
    pub fn intervals(&self) -> Option<Intervals> {
        self.broker.console_view().intervals(&self.caller)
    }

    /// The names the Access page says (super users, flag users), if the console may describe the cluster.
    #[must_use]
    pub fn access_facts(&self) -> Option<AccessFacts> {
        self.broker.console_view().access(&self.caller)
    }

    /// The size of the data directory. The walk reads the disk, so it runs off the async threads.
    pub async fn data_size(&self) -> Option<DataSize> {
        let broker = std::sync::Arc::clone(&self.broker);
        let caller = self.caller.clone();
        tokio::task::spawn_blocking(move || broker.console_view().data_size(&caller))
            .await
            .ok()
            .flatten()
    }

    /// The APIs ApiVersions answers now: what the profile offers, narrowed to what this broker serves, with the two SASL
    /// APIs when SASL is on.
    #[must_use]
    pub fn apis_offered(&self, sasl: bool) -> Vec<Advertised> {
        self.profile.advertised(Features {
            sasl,
            telemetry: self.broker.client_telemetry(),
            served: self.broker.served(),
        })
    }

    /// Every API this Kafka version has, served or not, SASL on and telemetry on: what "not offered" is measured against.
    #[must_use]
    pub fn apis_of_the_version(&self) -> Vec<Advertised> {
        self.profile.advertised(Features {
            sasl: true,
            telemetry: true,
            served: ApiSet::ALL,
        })
    }
}
