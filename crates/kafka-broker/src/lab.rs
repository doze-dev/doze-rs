//! What the Lab asks of the broker besides requests: today, only the name behind a topic id (a fault rule that names a topic is
//! matched against a Fetch or Produce from v13, which names its topics by id). The forced rebalance, expiry and cleaning round of the
//! console are in `view` (feature `inspect`).

use kafka_storage::fs::Fs;
use kafka_wire::ids::TopicId;

use crate::Broker;

impl<F: Fs> Broker<F> {
    /// The name of the topic with this id, if there is one.
    #[must_use]
    pub fn topic_name(&self, id: kafka_wire::Uuid) -> Option<String> {
        let topic = self.meta.topic_by_id(TopicId::from_uuid(id)?)?;
        Some(topic.name.as_ref().to_owned())
    }
}
