//! The words of a rule: its sentence (`Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes`), the short phrases a table
//! cell holds, and the name a rule gets when it is given none. Made once from the rule; never a live number in them.

use std::time::Duration;

use super::rule::{Effect, Fail, LONGEST_NAME, Share, Spec};

/// A list as words: `a`, `a and b`, `a, b and c`; past three, how many (`4 services`).
fn list(items: &[String], many: &str) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        more => format!("{} {many}", more.len()),
    }
}

/// How often, as words for a table: `every call`, `1 in 4`, `30 %`.
#[must_use]
pub fn share_words(share: Share) -> String {
    match share {
        Share::Every => "every call".to_owned(),
        Share::Nth(n) => format!("1 in {n}"),
        Share::Percent(p) => format!("{p} %"),
    }
}

/// How long, as the largest whole unit: `1 second`, `90 seconds`, `10 minutes`, `2 hours`.
#[must_use]
pub fn duration_words(d: Duration) -> String {
    let s = d.as_secs();
    let (n, unit) = if s >= 3600 && s.is_multiple_of(3600) {
        (s / 3600, "hour")
    } else if s >= 60 && s.is_multiple_of(60) {
        (s / 60, "minute")
    } else {
        (s, "second")
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// The services and operations of a rule, as words (`SQS SendMessage`, `SQS and SNS`); empty when it names neither. `title` turns
/// the name of a service as the rule holds it into the one a person reads.
#[must_use]
pub fn what(spec: &Spec, title: &dyn Fn(&str) -> String) -> String {
    let m = &spec.matcher;
    let services: Vec<String> = m.services.iter().map(|s| title(s)).collect();
    let ops: Vec<String> = m.operations.iter().map(ToString::to_string).collect();
    let mut out = list(&services, "services");
    let ops = list(&ops, "operations");
    if !out.is_empty() && !ops.is_empty() {
        out.push(' ');
    }
    out.push_str(&ops);
    out
}

/// What a rule fits the calls on, as words: `orders`, `orders and orders-dlq`; `every resource` when it names none.
#[must_use]
pub fn target(spec: &Spec) -> String {
    let patterns: Vec<String> = spec
        .matcher
        .resources
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if patterns.is_empty() {
        "every resource".to_owned()
    } else {
        list(&patterns, "patterns")
    }
}

fn effect_phrase(effect: &Effect) -> String {
    match effect {
        Effect::Slow { .. } => "Delay".to_owned(),
        Effect::Fail(Fail::Throttle) => "Throttle".to_owned(),
        Effect::Fail(Fail::Unavailable) => "Answer unavailable (503) to".to_owned(),
        Effect::Fail(Fail::Internal) => "Answer an internal error (500) to".to_owned(),
        Effect::Fail(Fail::AccessDenied) => "Deny".to_owned(),
        Effect::Fail(Fail::Custom { status, code, .. }) => format!("Answer {code} ({status}) to"),
        Effect::Drop => "Close the connection of".to_owned(),
    }
}

/// The calls a rule fits, as a noun phrase: `1 in 4 SQS SendMessage calls to orders in eu-west-1`.
fn subject(spec: &Spec, title: &dyn Fn(&str) -> String) -> String {
    let named = what(spec, title);
    let plural = !matches!(spec.share, Share::Every);
    let calls = if plural { "calls" } else { "call" };
    let mut out = match spec.share {
        Share::Every => "every ".to_owned(),
        Share::Nth(n) => format!("1 in {n} "),
        Share::Percent(p) => format!("{p} % of "),
    };
    if !named.is_empty() {
        out.push_str(&named);
        out.push(' ');
    }
    out.push_str(calls);
    let m = &spec.matcher;
    if !m.resources.is_empty() {
        out.push_str(" to ");
        out.push_str(&target(spec));
    }
    if !m.regions.is_empty() {
        let regions: Vec<String> = m.regions.iter().map(ToString::to_string).collect();
        out.push_str(" in ");
        out.push_str(&list(&regions, "regions"));
    }
    if !m.callers.is_empty() {
        let keys: Vec<String> = m.callers.iter().map(ToString::to_string).collect();
        out.push_str(" from ");
        out.push_str(&list(&keys, "keys"));
    }
    out
}

/// What the rule does, in a sentence.
#[must_use]
pub fn sentence(spec: &Spec, title: &dyn Fn(&str) -> String) -> String {
    let mut out = format!("{} {}", effect_phrase(&spec.effect), subject(spec, title));
    if let Effect::Slow {
        delay_ms,
        jitter_ms,
    } = spec.effect
    {
        out.push_str(&format!(" by {delay_ms} ms"));
        if jitter_ms > 0 {
            out.push_str(&format!(" and up to {jitter_ms} ms more"));
        }
    }
    out.push_str(" for ");
    out.push_str(&duration_words(spec.ttl));
    match spec.max_hits {
        Some(1) => out.push_str(", once"),
        Some(n) => out.push_str(&format!(", at most {n} times")),
        None => {}
    }
    out
}

/// The name a rule goes by when it is given none: what it does and to what, cut to the longest a name may be.
#[must_use]
pub fn default_name(spec: &Spec) -> String {
    let named = what(spec, &ToString::to_string);
    let mut out = match &spec.effect {
        Effect::Slow { delay_ms, .. } => format!("Delay {delay_ms} ms"),
        Effect::Fail(Fail::Throttle) => "Throttle".to_owned(),
        Effect::Fail(Fail::Unavailable) => "Unavailable".to_owned(),
        Effect::Fail(Fail::Internal) => "Internal error".to_owned(),
        Effect::Fail(Fail::AccessDenied) => "Access denied".to_owned(),
        Effect::Fail(Fail::Custom { code, .. }) => format!("Fail with {code}"),
        Effect::Drop => "Drop".to_owned(),
    };
    out.push(' ');
    out.push_str(if named.is_empty() {
        "every call"
    } else {
        &named
    });
    out.chars().take(LONGEST_NAME).collect()
}

#[cfg(test)]
mod tests;
