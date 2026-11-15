//! What the Data page asks of the source: the usage, what Empty topic will remove, and the three actions.

use kafka_broker::{DataRefusal, Emptied, EmptiedAll, Erased};

use super::Source;
use crate::gather;
use crate::model::{DataView, DiskAlarm, EmptyTopicView};

/// What Erase everything did: the broker's part, and the registry's (the subjects it removed, or why it could not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErasedAll {
    pub broker: Erased,
    /// `None`: there is no registry. `Some(Err)`: the broker was erased and the registry refused.
    pub subjects: Option<Result<usize, String>>,
}

impl Source {
    /// The subjects the registry lists, if there is one.
    fn subjects(&self) -> Option<usize> {
        self.schemas()
            .registry()
            .map(|r| r.subjects("", false, false).len())
    }

    /// The Data page; `None` if the console may not describe the cluster.
    pub async fn data(&self) -> Option<DataView> {
        match self {
            Self::Live(l) => gather::data::view(&l.backend, &l.growth, self.subjects()).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => Some(crate::fixtures_data::view()),
        }
    }

    /// The heads-up the latest look at the data directory called for, without looking again (the top bar asks on every page).
    #[must_use]
    pub fn disk_alarm(&self) -> Option<DiskAlarm> {
        match self {
            Self::Live(l) => l.growth.alarm(),
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_data::view().alarm,
        }
    }

    /// What Empty topic will remove from `topic`; `None` if there is no such topic.
    pub async fn empty_form(&self, topic: &str) -> Option<EmptyTopicView> {
        match self {
            Self::Live(l) => gather::data::empty_form(&l.backend, topic).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_data::empty_form(topic),
        }
    }

    /// Empties `topic`.
    ///
    /// # Errors
    /// The broker's refusal.
    pub async fn empty_topic(&self, topic: &str) -> Result<Emptied, DataRefusal> {
        match self {
            Self::Live(l) => l.backend.empty_topic(topic).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_data::empty_topic(topic),
        }
    }

    /// Empties every topic; the growth measured so far is let go.
    ///
    /// # Errors
    /// The broker's refusal.
    pub async fn empty_all(&self) -> Result<EmptiedAll, DataRefusal> {
        match self {
            Self::Live(l) => {
                let done = l.backend.empty_topics().await;
                l.growth.forget();
                done
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => Ok(crate::fixtures_data::emptied_all()),
        }
    }

    /// Erases the broker's topics, groups and transactional ids, then the registry's subjects.
    ///
    /// # Errors
    /// The broker's refusal (the registry is not touched then).
    pub async fn erase(&self) -> Result<ErasedAll, DataRefusal> {
        let broker = match self {
            Self::Live(l) => {
                let done = l.backend.erase_everything().await;
                l.growth.forget();
                done?
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_data::erased()?,
        };
        let subjects = self
            .schemas()
            .registry()
            .map(|r| r.erase().map(|e| e.subjects).map_err(|e| e.to_string()));
        Ok(ErasedAll { broker, subjects })
    }
}
