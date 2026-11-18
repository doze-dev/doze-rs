//! What the Consumers tab of a topic asks of the source.

use super::Source;
use crate::gather;
use crate::model::ConsumersView;

impl Source {
    /// The groups that have committed offsets on the topic, each with its lag on it; `None` if there is no such topic or the
    /// console may not describe it.
    pub async fn consumers(&self, name: &str) -> Option<ConsumersView> {
        match self {
            Self::Live(l) => gather::consumers::consumers(&l.backend, name).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_groups::consumers(name),
        }
    }
}
