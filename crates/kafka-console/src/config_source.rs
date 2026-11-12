//! Where the Config pages get their answers: the live broker (DescribeConfigs and IncrementalAlterConfigs through
//! `Broker::handle`, as the console's principal) or, in the tests and the `boards` example, the larder.

use crate::gather::{self, ConfigFields, ConfigMissing};
use crate::model::{
    ConfigAsk, ConfigEditView, ConfigPage, ConfigResetResult, ConfigResetView, ConfigResult,
};
use crate::source::Source;

impl Source {
    /// A topic's Config page; `None` if there is no such topic.
    pub async fn config_page(&self, topic: &str, ask: ConfigAsk) -> Option<ConfigPage> {
        match self {
            Self::Live(l) => gather::config_page(&l.backend, topic, ask).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_config::page(topic, ask),
        }
    }

    /// The edit dialog for a config of a topic.
    ///
    /// # Errors
    /// [`ConfigMissing`] if the topic or the config is not there.
    pub async fn config_form(
        &self,
        topic: &str,
        fields: ConfigFields<'_>,
    ) -> Result<ConfigEditView, ConfigMissing> {
        match self {
            Self::Live(l) => gather::config_form(&l.backend, topic, fields).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_config::form(topic, fields),
        }
    }

    /// Saves the form, or with `check` asks the broker whether it would take it.
    pub async fn config_save(
        &self,
        topic: &str,
        fields: ConfigFields<'_>,
        check: bool,
    ) -> ConfigResult {
        match self {
            Self::Live(l) => gather::config_save(&l.backend, topic, fields, check).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_config::save(topic, fields, check),
        }
    }

    /// The reset dialog for a config of a topic.
    ///
    /// # Errors
    /// [`ConfigMissing`] if the topic or the config is not there.
    pub async fn config_reset_form(
        &self,
        topic: &str,
        key: &str,
    ) -> Result<ConfigResetView, ConfigMissing> {
        match self {
            Self::Live(l) => gather::config_reset_form(&l.backend, topic, key).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_config::reset_form(topic, key),
        }
    }

    /// Removes the value the topic sets for `key`.
    pub async fn config_reset(&self, topic: &str, key: &str) -> ConfigResetResult {
        match self {
            Self::Live(l) => gather::config_reset(&l.backend, topic, key).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_config::reset(topic, key),
        }
    }
}
