//! Reading what Kafka said about a value, to try the values that might pass, and working out what a list becomes. Pure
//! functions. Nothing here decides whether a value is allowed: a candidate is only ever shown after the broker has
//! accepted it with `validate_only`.

use crate::model::ConfigOp;

const ONE_OF: &str = "must be one of: ";
const BOUNDS: [&str; 2] = ["must be at least ", "must be no more than "];

/// Where the last `marker` is in `message` and what follows it: Kafka's own reason comes last, after the value typed.
fn after<'a>(message: &'a str, marker: &str) -> Option<(usize, &'a str)> {
    let at = message.rfind(marker)?;
    Some((at, message.get(at + marker.len()..)?))
}

/// The values Kafka's refusal names: the choices of "must be one of: a, b", the bound of "must be at least N" and of
/// "must be no more than N". Any other message names none.
#[must_use]
pub fn options(message: &str) -> Vec<String> {
    let bound = BOUNDS
        .iter()
        .filter_map(|key| after(message, key))
        .max_by_key(|(at, _)| *at);
    let one_of = after(message, ONE_OF);
    if let Some((_, rest)) = one_of.filter(|(at, _)| bound.is_none_or(|(b, _)| *at > b)) {
        return rest
            .split(',')
            .map(|o| o.trim().to_owned())
            .filter(|o| !o.is_empty() && o.len() <= 40)
            .take(8)
            .collect();
    }
    bound
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .map(|n| n.trim_end_matches('.').to_owned())
        .filter(|n| n.parse::<f64>().is_ok())
        .into_iter()
        .collect()
}

fn elements(list: &str) -> Vec<&str> {
    list.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect()
}

/// What a list config becomes when `op` is applied to `current` with `value`: the elements of `value` are added after the
/// ones there (once each) or taken out; SET replaces the list; DELETE leaves nothing of the topic's own.
#[must_use]
pub fn after_of(current: &str, op: ConfigOp, value: &str) -> String {
    match op {
        ConfigOp::Set => value.trim().to_owned(),
        ConfigOp::Delete => String::new(),
        ConfigOp::Append => {
            let mut all = elements(current);
            for e in elements(value) {
                if !all.contains(&e) {
                    all.push(e);
                }
            }
            all.join(",")
        }
        ConfigOp::Subtract => {
            let gone = elements(value);
            elements(current)
                .into_iter()
                .filter(|e| !gone.contains(e))
                .collect::<Vec<_>>()
                .join(",")
        }
    }
}

#[cfg(test)]
mod tests;
