//! The five dynamic broker configs of the Settings page: DescribeConfigs of the BROKER resource read into rows, and the
//! topics that follow a broker default (DescribeConfigs of each topic's own value).

use kafka_wire::api::Response;
use kafka_wire::messages::describe_configs_response::DescribeConfigsResourceResult;

use crate::backend::{Backend, requests::broker_configs};
use crate::model::{ConfigSource, SettingsConfigRow, Span};
use crate::settings_words::{DYNAMIC, dynamic_flag, gloss, seen_as, takes_effect, topic_key, what};

/// Everything the broker lists for itself: the five, and the ones it holds and does not change while it runs.
#[derive(Debug, Clone, Default)]
pub struct BrokerConfigs {
    pub rows: Vec<SettingsConfigRow>,
    /// The configs that are read only, as `(name, value)`.
    pub fixed: Vec<(String, String)>,
}

/// The source of a row in the Settings table's words.
#[must_use]
pub fn source_label(s: ConfigSource) -> &'static str {
    match s {
        ConfigSource::DynamicBroker => "Dynamic broker",
        ConfigSource::DynamicDefault => "Dynamic default broker",
        ConfigSource::StaticFlag => "Static broker",
        ConfigSource::Default => "Default",
        ConfigSource::DynamicTopic => "Dynamic topic",
        ConfigSource::Other(_) => "Other",
    }
}

fn text_of(v: &Option<kafka_wire::StrBytes>) -> String {
    v.as_ref().map(ToString::to_string).unwrap_or_default()
}

/// Where a row's value comes from, in words: the scope of a dynamic value, the flag and Kafka's default, or Kafka's default.
fn note_of(key: &str, c: &DescribeConfigsResourceResult, source: ConfigSource) -> String {
    let kafka_default = c
        .synonyms
        .iter()
        .find(|s| s.source == 5)
        .map(|s| text_of(&s.value))
        .map(|v| gloss(key, &v).unwrap_or(v));
    match source {
        ConfigSource::DynamicBroker => "set for this broker".to_owned(),
        ConfigSource::DynamicDefault => "set for all brokers".to_owned(),
        ConfigSource::StaticFlag => format!(
            "flag {}{}",
            dynamic_flag(key).unwrap_or("at start"),
            kafka_default.map_or_else(String::new, |d| format!("; Kafka's default is {d}"))
        ),
        _ => "Kafka's default".to_owned(),
    }
}

fn row_of(c: &DescribeConfigsResourceResult, retention_secs: Option<u64>) -> SettingsConfigRow {
    let key = c.name.to_string();
    let value = text_of(&c.value);
    let source = ConfigSource::from_wire(c.config_source);
    SettingsConfigRow {
        what: what(&key).to_owned(),
        gloss: gloss(&key, &value),
        source_note: note_of(&key, c, source),
        takes_effect: takes_effect(&key, retention_secs),
        name: key,
        value,
        source,
    }
}

/// The broker's configs, as it lists them for `node`; `None` if it did not answer or refused.
pub async fn of_broker(
    b: &Backend,
    node: i32,
    retention_secs: Option<u64>,
) -> Option<BrokerConfigs> {
    let Ok(Response::DescribeConfigs(r)) = b.call(broker_configs::describe_broker(node)).await
    else {
        return None;
    };
    let res = r.results.first().filter(|r| r.error_code == 0)?;
    let mut out = BrokerConfigs::default();
    for (key, _, _) in DYNAMIC {
        if let Some(c) = res.configs.iter().find(|c| c.name.as_str() == key) {
            out.rows.push(row_of(c, retention_secs));
        }
    }
    out.fixed = res
        .configs
        .iter()
        .filter(|c| c.read_only && !c.is_sensitive)
        .map(|c| (c.name.to_string(), text_of(&c.value)))
        .collect();
    Some(out)
}

/// How many of the rows are set at run time.
#[must_use]
pub fn set_count(rows: &[SettingsConfigRow]) -> usize {
    rows.iter()
        .filter(|r| {
            matches!(
                r.source,
                ConfigSource::DynamicBroker | ConfigSource::DynamicDefault
            )
        })
        .count()
}

/// The topics, split by what they do about one broker default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Followers {
    /// Topics that set none of their own and so follow the broker.
    pub following: Vec<String>,
    /// Topics that keep their own value, with it.
    pub own: Vec<(String, String)>,
    /// Topics that only compact: retention does not apply to them.
    pub compact_only: Vec<String>,
    /// Topics asked about, and how many there are in all.
    pub asked: usize,
    pub total: usize,
}

/// Which topics follow the broker default `key`, by asking each for its own value (DescribeConfigs, at most 200).
pub async fn followers(b: &Backend, key: &str) -> Followers {
    let Some(topic) = topic_key(key) else {
        return Followers::default();
    };
    let Ok(Response::Metadata(m)) = b.call(crate::backend::requests::metadata_all()).await else {
        return Followers::default();
    };
    let mut names: Vec<String> = m
        .topics
        .iter()
        .filter(|t| !t.is_internal)
        .filter_map(|t| t.name.as_ref().map(|n| n.0.to_string()))
        .collect();
    names.sort();
    let total = names.len();
    let Ok(Response::DescribeConfigs(r)) = b
        .call(broker_configs::describe_topics(
            &names,
            &[topic, "cleanup.policy"],
        ))
        .await
    else {
        return Followers::default();
    };
    let mut out = Followers {
        asked: names.len().min(broker_configs::MOST_TOPICS),
        total,
        ..Followers::default()
    };
    for res in r.results.iter().filter(|r| r.error_code == 0) {
        let name = res.resource_name.to_string();
        let find = |k: &str| res.configs.iter().find(|c| c.name.as_str() == k);
        let own = find(topic).filter(|c| c.config_source == 1);
        let retention = topic.starts_with("retention.");
        if retention
            && find("cleanup.policy").is_some_and(|c| text_of(&c.value).trim() == "compact")
        {
            out.compact_only.push(name);
        } else if let Some(c) = own {
            out.own.push((name, text_of(&c.value)));
        } else {
            out.following.push(name);
        }
    }
    out
}

fn list(names: &[String]) -> String {
    const MOST: usize = 6;
    let shown = names
        .iter()
        .take(MOST)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > MOST {
        format!("{shown} and {} more", names.len() - MOST)
    } else {
        shown
    }
}

/// The names as a sentence says them: `a`, `a and b`, `a, b and c`, and past six `a, b, c, d, e, f and 3 more`.
fn prose(names: &[String]) -> String {
    const MOST: usize = 6;
    let shown = names.get(..MOST).unwrap_or(names);
    match (shown.split_last(), names.len() > MOST) {
        (None, _) => String::new(),
        (Some(_), true) => format!("{} and {} more", shown.join(", "), names.len() - MOST),
        (Some((last, [])), false) => last.clone(),
        (Some((last, head)), false) => format!("{} and {last}", head.join(", ")),
    }
}

fn topics(n: usize) -> String {
    if n == 1 {
        "1 topic".to_owned()
    } else {
        format!("{n} topics")
    }
}

/// The "who sees it" lines of the dialog: the topics with no value of their own, those that keep theirs, those retention
/// does not apply to. `own_key` is the topic config's name (`retention.ms`).
#[must_use]
pub fn who_sees(key: &str, f: &Followers) -> (Vec<Span>, String) {
    let own_key = topic_key(key).unwrap_or(key);
    if f.total == 0 {
        return (
            vec![Span::text(
                "no topic yet: every topic made from now on starts from this value.",
            )],
            String::new(),
        );
    }
    let mut main = vec![
        Span::text(format!("the topics with no {own_key} of their own: ")),
        Span::mono(list(&f.following)),
        Span::text("."),
    ];
    if f.following.is_empty() {
        main = vec![Span::text(format!(
            "no topic follows it: every topic sets its own {own_key}."
        ))];
    }
    let mut dim = String::new();
    if !f.own.is_empty() {
        let kept: Vec<String> = f
            .own
            .iter()
            .map(|(n, v)| format!("{n} ({})", seen_as(key, v)))
            .collect();
        dim.push_str(&format!("{} keep theirs. ", prose(&kept)));
    }
    if !f.compact_only.is_empty() {
        dim.push_str(&format!(
            "{} {} compact only: retention does not apply. ",
            prose(&f.compact_only),
            if f.compact_only.len() == 1 {
                "is"
            } else {
                "are"
            }
        ));
    }
    if f.total > f.asked {
        dim.push_str(&format!(
            "Only the first {} of {} topics were looked at.",
            f.asked, f.total
        ));
    }
    (main, dim.trim().to_owned())
}

/// The sentence of the saved page's card: how many topics now see the value.
#[must_use]
pub fn who_now_sees(key: &str, value: &str, f: &Followers) -> Vec<Span> {
    let own_key = topic_key(key).unwrap_or(key);
    if f.following.is_empty() {
        return vec![Span::text(format!(
            "No topic follows the broker's {own_key}: each of the {} sets its own, so nothing changes for them.",
            topics(f.total)
        ))];
    }
    vec![
        Span::text(format!(
            "{} {} no {own_key} of their own and now follow ",
            topics(f.following.len()),
            if f.following.len() == 1 {
                "has"
            } else {
                "have"
            }
        )),
        Span::strong(seen_as(key, value)),
        Span::text(": "),
        Span::mono(list(&f.following)),
        Span::text(". The topics that set their own are unchanged."),
    ]
}

#[cfg(test)]
mod tests;
