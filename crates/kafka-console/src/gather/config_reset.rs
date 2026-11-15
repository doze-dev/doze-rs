//! Reset to default: IncrementalAlterConfigs DELETE of the value the topic sets, so the topic follows the broker again.

use kafka_wire::messages::ApiKey;

use super::config::{ConfigMissing, pace, rows};
use super::config_edit::send;
use crate::backend::Backend;
use crate::config_calls;
use crate::model::{ConfigOp, ConfigResetResult, ConfigResetView, ConfigValue};
use crate::refusal::refusal;

async fn view(b: &Backend, topic: &str, key: &str) -> Result<ConfigResetView, ConfigMissing> {
    let all = rows(b, topic).await.ok_or(ConfigMissing::Topic)?;
    let row = all
        .into_iter()
        .find(|r| r.name == key)
        .ok_or(ConfigMissing::Key)?;
    let after = row.fallback().map(|s| ConfigValue {
        value: s.value.clone(),
        source: s.source,
    });
    let call = config_calls::alter(
        b.version(ApiKey::IncrementalAlterConfigs),
        topic,
        &[(row.name.clone(), ConfigOp::Delete, None)],
    );
    Ok(ConfigResetView {
        topic: topic.to_owned(),
        row,
        after,
        pace: pace(b),
        call,
        refusal: None,
    })
}

/// The reset dialog for a config of a topic.
///
/// # Errors
/// [`ConfigMissing`] if the topic or the config is not there.
pub async fn form(b: &Backend, topic: &str, key: &str) -> Result<ConfigResetView, ConfigMissing> {
    view(b, topic, key).await
}

/// Removes the value the topic sets for `key`.
pub async fn reset(b: &Backend, topic: &str, key: &str) -> ConfigResetResult {
    let mut v = match view(b, topic, key).await {
        Ok(v) => v,
        Err(ConfigMissing::Topic) => return ConfigResetResult::NoTopic,
        Err(ConfigMissing::Key) => return ConfigResetResult::NoKey,
    };
    let change = [(v.row.name.clone(), ConfigOp::Delete, None)];
    match send(b, topic, &change, false).await {
        Some((0, _)) => ConfigResetResult::Done,
        Some((code, message)) => {
            v.refusal = Some(refusal(code, message.as_deref()));
            ConfigResetResult::Refused(Box::new(v))
        }
        None => ConfigResetResult::NoTopic,
    }
}
