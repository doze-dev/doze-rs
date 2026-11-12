//! The forms of the Lab: what was typed or chosen, as fields; the rule it makes, or the sentences that say what to fix; and what the rule
//! would do, worked out without making it. Pure: nothing here reads the broker or a clock.

use std::time::Duration;

use kafka_server::fault::{AddError, Effect, LONGEST_DELAY_MS, Matcher, Pattern, Spec};
use kafka_wire::messages::ApiKey;

use super::words::{self, CODES, ENDS, ttl_words};
use crate::format::number;
use crate::model::lab::{LabFields, LabKind, LabPreview};
use crate::recorder::errors;

/// The longest text a field is read for.
const MOST_TEXT: usize = 249;
/// The most hits a rule may be asked to stop after.
const MOST_HITS: u32 = 1_000_000;

/// The APIs the forms offer: the board's two.
pub const APIS: [(&str, &str, ApiKey); 2] = [
    ("produce", "Produce", ApiKey::Produce),
    ("fetch", "Fetch", ApiKey::Fetch),
];

fn cut(s: &str) -> String {
    s.trim().chars().take(MOST_TEXT).collect()
}

/// What a form carried: every field as text, cut to a length no rule needs.
#[must_use]
pub fn fields_of(f: &[(String, String)]) -> LabFields {
    let get = |k: &str| cut(crate::form::text(f, k));
    LabFields {
        api: get("api"),
        topic: get("topic"),
        client: get("client"),
        delay: get("delay"),
        jitter: get("jitter"),
        share: get("share"),
        ends: get("ends"),
        hits: get("hits"),
        code: get("code"),
    }
}

/// The form as it opens for a kind of rule: the board's own defaults.
#[must_use]
pub fn blank(kind: LabKind) -> LabFields {
    let mut f = LabFields {
        api: "produce".into(),
        delay: "400".into(),
        share: "4".into(),
        ends: "10".into(),
        ..LabFields::default()
    };
    if kind == LabKind::Fail {
        f.api = "fetch".into();
        f.share = "5".into();
        f.ends = "5".into();
        f.code = "7".into();
        f.delay.clear();
    }
    f
}

/// A field left out takes the form's default, so a link with half the fields (a "Run again") opens a whole form.
#[must_use]
pub fn filled(kind: LabKind, mut f: LabFields) -> LabFields {
    let d = blank(kind);
    for (now, was) in [
        (&mut f.api, d.api),
        (&mut f.delay, d.delay),
        (&mut f.share, d.share),
        (&mut f.ends, d.ends),
        (&mut f.code, d.code),
    ] {
        if now.is_empty() {
            *now = was;
        }
    }
    f
}

type Errors = Vec<(String, String)>;

fn err(field: &str, text: &str) -> (String, String) {
    (field.to_owned(), text.to_owned())
}

fn whole(text: &str, least: u32, most: u32) -> Option<u32> {
    text.parse::<u32>()
        .ok()
        .filter(|n| (least..=most).contains(n))
}

fn api_of(f: &LabFields) -> Result<ApiKey, (String, String)> {
    APIS.iter()
        .find(|(w, _, _)| *w == f.api)
        .map(|(_, _, k)| *k)
        .ok_or_else(|| err("api", "Choose Produce or Fetch."))
}

fn topic_of(f: &LabFields) -> Result<Vec<Pattern>, (String, String)> {
    if f.topic.is_empty() {
        return Ok(Vec::new());
    }
    Pattern::new(&f.topic).map(|p| vec![p]).ok_or_else(|| {
        err(
            "topic",
            "Choose a topic, or leave it empty for every topic.",
        )
    })
}

fn client_of(f: &LabFields) -> Result<Option<Pattern>, (String, String)> {
    if f.client.is_empty() {
        return Ok(None);
    }
    Pattern::new(&f.client).map(Some).ok_or_else(|| {
        err(
            "client",
            "A client.id is 1 to 249 characters with no control character. * and ? stand for any text and any one character.",
        )
    })
}

fn delay_of(f: &LabFields, errors: &mut Errors) -> Option<(u32, u32)> {
    let delay = whole(&f.delay, 1, LONGEST_DELAY_MS);
    if delay.is_none() {
        errors.push(err(
            "delay",
            &format!(
                "Type a delay from 1 to {} ms.",
                number(i64::from(LONGEST_DELAY_MS))
            ),
        ));
    }
    let jitter = if f.jitter.is_empty() {
        Some(0)
    } else {
        whole(&f.jitter, 0, LONGEST_DELAY_MS)
    };
    if jitter.is_none() {
        errors.push(err(
            "jitter",
            "Type the jitter in ms, from 0 to 60,000, or leave it empty.",
        ));
    }
    delay.zip(jitter)
}

fn code_of(f: &LabFields, errors: &mut Errors) -> Option<i16> {
    let code = f
        .code
        .parse::<i16>()
        .ok()
        .filter(|c| CODES.iter().any(|(k, _)| k == c));
    if code.is_none() {
        errors.push(err("code", "Choose one of the errors."));
    }
    code
}

fn collect<T>(r: Result<T, (String, String)>, errors: &mut Errors) -> Option<T> {
    r.map_err(|e| errors.push(e)).ok()
}

/// The rule a form asks for, or the sentence for each field to fix.
///
/// # Errors
/// One `(field, sentence)` for each field that is wrong.
pub fn spec_of(kind: LabKind, f: &LabFields) -> Result<Spec, Errors> {
    let mut errors = Errors::new();
    let api = collect(api_of(f), &mut errors);
    let topics = collect(topic_of(f), &mut errors);
    let client = collect(client_of(f), &mut errors);
    let share = words::share_of(&f.share);
    if share.is_none() {
        errors.push(err("share", "Choose how many of the requests."));
    }
    let ends = f
        .ends
        .parse::<u64>()
        .ok()
        .filter(|m| ENDS.iter().any(|(k, _)| k == m));
    if ends.is_none() {
        errors.push(err("ends", "Choose when the rule ends."));
    }
    let max_hits = if f.hits.is_empty() {
        Some(None)
    } else {
        whole(&f.hits, 1, MOST_HITS).map(Some)
    };
    if max_hits.is_none() {
        errors.push(err(
            "hits",
            "Type a number of hits from 1, or leave it empty.",
        ));
    }
    let effect = match kind {
        LabKind::Slow => delay_of(f, &mut errors).map(|(delay_ms, jitter_ms)| Effect::Slow {
            delay_ms,
            jitter_ms,
        }),
        LabKind::Fail => code_of(f, &mut errors).map(|code| Effect::Fail { code }),
        LabKind::Drop => Some(Effect::Drop),
    };
    let (
        Some(api),
        Some(topics),
        Some(client),
        Some(share),
        Some(ends),
        Some(max_hits),
        Some(effect),
    ) = (api, topics, client, share, ends, max_hits, effect)
    else {
        return Err(errors);
    };
    let mut spec = Spec {
        name: String::new(),
        matcher: Matcher {
            client,
            apis: vec![api],
            topics,
            ..Matcher::default()
        },
        effect,
        share,
        ttl: Duration::from_secs(ends * 60),
        max_hits,
    };
    spec.name = words::default_name(&spec);
    Ok(spec)
}

/// The field a refusal of the engine belongs to, if it belongs to one.
#[must_use]
pub fn field_of_refusal(e: AddError) -> Option<&'static str> {
    match e {
        AddError::BadDelay => Some("delay"),
        AddError::BadShare => Some("share"),
        AddError::BadCode => Some("code"),
        AddError::BadHits => Some("hits"),
        AddError::BadPartition | AddError::TooManyTopics => Some("topic"),
        AddError::FailNeedsApi | AddError::CannotFail(_) => Some("api"),
        AddError::BadName | AddError::BadDuration | AddError::TooMany => None,
    }
}

/// What the rule would do, worked out from the rule alone.
#[must_use]
pub fn preview(spec: &Spec) -> LabPreview {
    let mut rule = vec![(
        "doze".to_owned(),
        match spec.effect {
            Effect::Slow { .. } => "SlowTopic",
            Effect::Fail { .. } => "FailTopic",
            Effect::Drop => "DropConnection",
        }
        .to_owned(),
    )];
    let mut line = |k: &str, v: String| rule.push((k.to_owned(), v));
    line("api", words::apis_words(&spec.matcher.apis));
    let topic = words::topics_words(&spec.matcher);
    line(
        "topic",
        if topic.is_empty() {
            "any".to_owned()
        } else {
            topic
        },
    );
    if let Some(c) = &spec.matcher.client {
        line("client", c.as_str().to_owned());
    }
    match spec.effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => {
            line("delay", format!("{delay_ms}ms"));
            if jitter_ms > 0 {
                line("jitter", format!("{jitter_ms}ms"));
            }
        }
        Effect::Fail { code } => line("error", errors::name(code)),
        Effect::Drop => {}
    }
    line("share", words::share_words(spec.share));
    if let Some(h) = spec.max_hits {
        line("hits", format!("at most {h}"));
    }
    line("ttl", ttl_words(spec.ttl));
    LabPreview {
        rule,
        sentence: words::sentence(spec),
        wire: words::wire_words(spec),
    }
}

#[cfg(test)]
mod tests;
