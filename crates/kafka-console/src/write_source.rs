//! Where the topic writes get their answers: the live broker (a protocol request through `Broker::handle`, as the
//! console's principal) or, in the tests and the `boards` example, the larder.

use crate::gather::{self, AddResult, DeleteResult, RecordsResult};
use crate::model::{
    AddView, CreateResult, CreateView, DeleteRecordsView, DeleteView, Draft, ProduceInput,
    ProduceResult, ProduceView, Step,
};
use crate::source::Source;

impl Source {
    /// A form nobody has touched: the broker's default partition count, the event log's values.
    pub async fn fresh_draft(&self) -> Draft {
        match self {
            Self::Live(l) => Draft::fresh(l.backend.limits().map_or(1, |x| x.default_partitions)),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => Draft::fresh(1),
        }
    }

    /// The Create-topic form at `wanted` (or the step an error sends it back to).
    pub async fn create_form(&self, draft: Draft, wanted: Step) -> CreateView {
        match self {
            Self::Live(l) => {
                let inputs = gather::create_inputs(&l.backend, &l.facts.bootstrap()).await;
                gather::landing_view(draft, wanted, &inputs)
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::create_landing(draft, wanted),
        }
    }

    /// Makes the topic, or with `validate_only` runs the checks and makes nothing.
    pub async fn create_topic(&self, draft: Draft, validate_only: bool) -> CreateResult {
        match self {
            Self::Live(l) => {
                gather::create(&l.backend, draft, validate_only, &l.facts.bootstrap()).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::create_topic(draft, validate_only),
        }
    }

    /// The Produce dialog for a topic; `None` if there is no such topic.
    pub async fn produce_form(&self, topic: &str, input: &ProduceInput) -> Option<ProduceView> {
        match self {
            Self::Live(l) => gather::produce_form(&l.backend, topic, input).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::produce_form(topic, input),
        }
    }

    /// Writes one record.
    pub async fn produce(&self, topic: &str, input: &ProduceInput) -> ProduceResult {
        match self {
            Self::Live(l) => gather::produce(&l.backend, topic, input).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::produce_send(topic, input),
        }
    }

    /// The Add-partitions dialog for a topic; `None` if there is no such topic.
    pub async fn add_form(&self, topic: &str, count: Option<&str>) -> Option<AddView> {
        match self {
            Self::Live(l) => gather::add_form(&l.backend, topic, count).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::add_form(topic, count),
        }
    }

    /// Raises the topic to `count` partitions.
    pub async fn add_partitions(&self, topic: &str, count: &str) -> AddResult {
        match self {
            Self::Live(l) => gather::add(&l.backend, topic, count).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::add_partitions(topic, count),
        }
    }

    /// The Delete-records dialog for a partition; `None` if the topic or the partition is not there.
    pub async fn delete_records_form(
        &self,
        topic: &str,
        partition: i32,
        offset: Option<&str>,
    ) -> Option<DeleteRecordsView> {
        match self {
            Self::Live(l) => {
                gather::delete_records_form(&l.backend, topic, partition, offset).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::delete_records_form(topic, partition, offset),
        }
    }

    /// Moves a partition's log start to `offset`.
    pub async fn delete_records(&self, topic: &str, partition: i32, offset: &str) -> RecordsResult {
        match self {
            Self::Live(l) => gather::delete_records(&l.backend, topic, partition, offset).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::delete_records(topic, partition, offset),
        }
    }

    /// The Delete-topic dialog; `None` if there is no such topic.
    pub async fn delete_form(&self, topic: &str, typed: &str) -> Option<DeleteView> {
        match self {
            Self::Live(l) => gather::delete_form(&l.backend, topic, typed).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::delete_form(topic, typed),
        }
    }

    /// Deletes the topic if `typed` is its name.
    pub async fn delete_topic(&self, topic: &str, typed: &str) -> DeleteResult {
        match self {
            Self::Live(l) => gather::delete(&l.backend, topic, typed).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_write::delete_topic(topic, typed),
        }
    }
}
