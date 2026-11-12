//! The edit dialog of a dynamic broker config and what a save comes to: IncrementalAlterConfigs on the BROKER resource (SET;
//! with `validate_only` a dry run that changes nothing), then DescribeConfigs again for what the configs are now.

use kafka_wire::api::Response;
use kafka_wire::messages::ApiKey;

use super::settings_configs::{
    BrokerConfigs, Followers, followers, of_broker, set_count, who_now_sees, who_sees,
};
use super::settings_refuse::{refused, said};
use crate::backend::{Backend, requests::broker_configs};
use crate::model::{
    SettingsChecked, SettingsEditView, SettingsLine, SettingsResult, SettingsSaved, SettingsScope,
    SettingsTeach, Span, Tok,
};
use crate::settings_words::{DYNAMIC, allowed, is_dynamic, takes_effect, unit};

/// What a form carried.
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsFields<'a> {
    /// The config the select names now.
    pub key: &'a str,
    /// The config the dialog was drawn for (the select may have moved since).
    pub was: &'a str,
    pub scope: &'a str,
    pub value: &'a str,
}

/// What the dialog is built from.
#[derive(Debug)]
pub struct World<'a> {
    pub node: i32,
    pub retention_secs: Option<u64>,
    pub configs: &'a BrokerConfigs,
    pub who: &'a Followers,
    pub version: i16,
}

/// The name of the BROKER resource a scope goes under: `""` is all brokers, the node id is this one.
#[must_use]
pub fn resource_name(scope: SettingsScope, node: i32) -> String {
    match scope {
        SettingsScope::All => String::new(),
        SettingsScope::This => node.to_string(),
    }
}

/// The call, as the boards draw it.
#[must_use]
pub fn call_lines(
    version: i16,
    scope: SettingsScope,
    node: i32,
    key: &str,
    value: &str,
) -> Vec<Vec<(Tok, String)>> {
    let who = match scope {
        SettingsScope::All => "\"\" (all brokers)".to_owned(),
        SettingsScope::This => format!("\"{node}\" (this broker)"),
    };
    vec![
        vec![
            (Tok::Keyword, "IncrementalAlterConfigs".to_owned()),
            (Tok::Plain, format!(" v{version}")),
        ],
        vec![(Tok::Plain, format!("  BROKER {who}"))],
        vec![(
            Tok::Plain,
            format!(
                "  SET {} = {}",
                crate::calls::visible(key, 60),
                crate::calls::visible(value, 80)
            ),
        )],
    ]
}

fn line(text: Vec<Span>, dim: impl Into<String>) -> SettingsLine {
    SettingsLine {
        text,
        dim: dim.into(),
    }
}

fn teach_of(w: &World<'_>, key: &str, scope: SettingsScope) -> SettingsTeach {
    let row = w.configs.rows.iter().find(|r| r.name == key);
    let now = row.map_or_else(Vec::new, |r| {
        let value = r
            .gloss
            .as_ref()
            .map_or_else(|| r.value.clone(), |g| format!("{}, {g}", r.value));
        vec![Span::text(value)]
    });
    let source = row.map_or_else(String::new, |r| {
        format!(
            "Source: {}, {}.",
            crate::gather::settings_configs::source_label(r.source).to_lowercase(),
            r.source_note
        )
    });
    let (who, dim) = who_sees(key, w.who);
    let applies = match scope {
        SettingsScope::All => "",
        SettingsScope::This => " This broker's own value wins over the one for all brokers.",
    };
    SettingsTeach {
        now: line(now, source),
        allowed: line(
            vec![Span::text(allowed(key))],
            match key {
                "log.retention.ms" | "log.retention.bytes" => "-1 keeps them for ever.",
                _ => "",
            },
        ),
        takes_effect: line(
            vec![Span::text(
                takes_effect(key, w.retention_secs).to_lowercase(),
            )],
            format!("No restart; the change is kept on disk.{applies}"),
        ),
        who_sees: line(who, dim),
    }
}

/// The dialog for the typed input (pure: everything it says is in the arguments).
#[must_use]
pub fn view_of(
    w: &World<'_>,
    key: &str,
    scope: SettingsScope,
    value: &str,
    state: (Option<String>, Option<SettingsChecked>),
) -> SettingsEditView {
    SettingsEditView {
        key: key.to_owned(),
        what: crate::settings_words::what(key).to_owned(),
        keys: DYNAMIC.iter().map(|d| d.0.to_owned()).collect(),
        scope,
        value: value.to_owned(),
        unit: unit(key, value),
        node_id: w.node,
        teach: teach_of(w, key, scope),
        call: call_lines(w.version, scope, w.node, key, value),
        error: state.0,
        checked: state.1,
    }
}

fn start_value(c: &BrokerConfigs, key: &str) -> String {
    c.rows
        .iter()
        .find(|r| r.name == key)
        .map(|r| r.value.clone())
        .unwrap_or_default()
}

async fn world_for<'a>(
    b: &Backend,
    configs: &'a BrokerConfigs,
    who: &'a Followers,
    node: i32,
) -> World<'a> {
    World {
        node,
        retention_secs: b.intervals().map(|i| i.retention_check.as_secs()),
        configs,
        who,
        version: b.version(ApiKey::IncrementalAlterConfigs),
    }
}

/// The node id of the only broker; 1 where the broker does not say.
fn node_of(b: &Backend) -> i32 {
    b.intervals().map_or(1, |i| i.node_id)
}

/// The dialog for `key`, starting from the value in force; `None` if `key` is not one of the five.
pub async fn form(b: &Backend, key: &str, scope: &str) -> Option<SettingsEditView> {
    if !is_dynamic(key) {
        return None;
    }
    let node = node_of(b);
    let retention = b.intervals().map(|i| i.retention_check.as_secs());
    let configs = of_broker(b, node, retention).await?;
    let who = followers(b, key).await;
    let w = world_for(b, &configs, &who, node).await;
    let value = start_value(&configs, key);
    Some(view_of(
        &w,
        key,
        SettingsScope::from_form(scope),
        &value,
        (None, None),
    ))
}

/// What one request to the broker answered: its code and its words.
pub(super) async fn send(
    b: &Backend,
    name: &str,
    key: &str,
    value: &str,
    validate_only: bool,
) -> Option<(i16, Option<String>)> {
    let Ok(Response::IncrementalAlterConfigs(r)) = b
        .call(broker_configs::alter_broker(
            name,
            key,
            value,
            validate_only,
        ))
        .await
    else {
        return None;
    };
    let a = r.responses.into_iter().next()?;
    Some((a.error_code, a.error_message.map(|m| m.to_string())))
}

/// Saves, or with `check` asks the broker whether it would take it and changes nothing. A config that is not one of the five
/// is only ever asked with `validate_only`: the console offers no such change, and shows what Kafka says to it.
pub async fn save(b: &Backend, f: SettingsFields<'_>, check: bool) -> SettingsResult {
    let scope = SettingsScope::from_form(f.scope);
    let node = node_of(b);
    let retention = b.intervals().map(|i| i.retention_check.as_secs());
    let version = b.version(ApiKey::IncrementalAlterConfigs);
    let Some(before) = of_broker(b, node, retention).await else {
        return refused(b, f, scope, node, None, true).await;
    };
    if !is_dynamic(f.key) {
        let (code, message) = send(b, &resource_name(scope, node), f.key, f.value, true)
            .await
            .unwrap_or((-1, None));
        return refused(b, f, scope, node, Some(said(code, message)), true).await;
    }
    let who = followers(b, f.key).await;
    let w = World {
        node,
        retention_secs: retention,
        configs: &before,
        who: &who,
        version,
    };
    if f.was != f.key {
        let value = start_value(&before, f.key);
        return SettingsResult::Switched(Box::new(view_of(&w, f.key, scope, &value, (None, None))));
    }
    let value = f.value.trim();
    if value.is_empty() {
        let e = Some("Type a value.".to_owned());
        return SettingsResult::Invalid(Box::new(view_of(&w, f.key, scope, value, (e, None))));
    }
    let Some((code, message)) = send(b, &resource_name(scope, node), f.key, value, check).await
    else {
        return refused(b, f, scope, node, None, check).await;
    };
    if code != 0 {
        return refused(b, f, scope, node, Some(said(code, message)), check).await;
    }
    if check {
        let checked = SettingsChecked {
            before: start_value(&before, f.key),
            after: value.to_owned(),
        };
        return SettingsResult::Checked(Box::new(view_of(
            &w,
            f.key,
            scope,
            value,
            (None, Some(checked)),
        )));
    }
    saved(b, f.key, scope, value, (node, retention, version)).await
}

async fn saved(
    b: &Backend,
    key: &str,
    scope: SettingsScope,
    value: &str,
    (node, retention, version): (i32, Option<u64>, i16),
) -> SettingsResult {
    let Some(now) = of_broker(b, node, retention).await else {
        return refused(
            b,
            SettingsFields {
                key,
                was: key,
                scope: scope.form(),
                value,
            },
            scope,
            node,
            None,
            false,
        )
        .await;
    };
    let who = followers(b, key).await;
    let mut call = call_lines(version, scope, node, key, value);
    call.push(vec![]);
    call.push(vec![(Tok::Plain, "-> 200, no error".to_owned())]);
    let row = now.rows.iter().find(|r| r.name == key);
    SettingsResult::Saved(Box::new(SettingsSaved {
        key: key.to_owned(),
        scope,
        value: value.to_owned(),
        gloss: crate::settings_words::gloss(key, value),
        set: set_count(&now.rows),
        who_sees: who_now_sees(key, value, &who),
        call,
        effect: row.map_or_else(String::new, |r| r.takes_effect.clone()),
        rows: now.rows,
    }))
}

#[cfg(test)]
mod tests;
