//! Which rule acts on a call, and what it remembers of it: the share gate, the weights that decide a winner, the record of a hit.

use aws_gateway::Ask;

use super::super::record::{First, Rule};
use super::super::rule::{Effect, MOST_KEPT_CALLERS, Share};
use super::{Inner, Live};

/// How much a firing rule outweighs another: a drop before a fail before a delay, the longer delay of two. For a delay the second
/// number is the wait in milliseconds, jitter drawn.
type Weight = (u8, u64);

/// Whether the share lets this call through.
fn gate(l: &mut Live, draw: u64) -> bool {
    match l.rule.spec.share {
        Share::Every => true,
        Share::Nth(n) => {
            if l.since >= n {
                l.since = 0;
                true
            } else {
                false
            }
        }
        Share::Percent(p) => (draw >> 8) % 100 < u64::from(p),
    }
}

fn weigh(effect: &Effect, draw: u64) -> Weight {
    match effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => {
            let extra = if *jitter_ms == 0 {
                0
            } else {
                (draw >> 16) % (u64::from(*jitter_ms) + 1)
            };
            (1, u64::from(*delay_ms) + extra)
        }
        Effect::Fail(_) => (2, 0),
        Effect::Drop => (3, 0),
    }
}

/// Counts the call against every rule that fits it, and picks the rule that acts.
pub(super) fn choose(inner: &mut Inner, ask: &Ask<'_>) -> Option<(usize, Weight)> {
    let mut best: Option<(usize, Weight)> = None;
    for i in 0..inner.rules.len() {
        let fits = inner
            .rules
            .get(i)
            .is_some_and(|l| l.rule.enabled && l.rule.spec.matcher.fits(ask));
        if !fits {
            continue;
        }
        let draw = inner.random();
        let Some(l) = inner.rules.get_mut(i) else {
            continue;
        };
        l.rule.matched += 1;
        l.since = l.since.saturating_add(1);
        if !gate(l, draw) {
            continue;
        }
        let w = weigh(&l.rule.spec.effect, draw);
        if best.is_none_or(|(_, b)| w > b) {
            best = Some((i, w));
        }
    }
    best
}

fn clip(text: &str, most: usize) -> Box<str> {
    text.chars().take(most).collect::<String>().into()
}

/// Puts the hit in the rule's record.
pub(super) fn record(rule: &mut Rule, ask: &Ask<'_>, wall: i64) {
    rule.hits += 1;
    rule.last_ms = Some(wall);
    let caller = clip(ask.access_key_id, 64);
    if rule.first.is_none() {
        rule.first = Some(First {
            at_ms: wall,
            caller: caller.clone(),
            service: ask.sdk_id,
            operation: clip(ask.operation, 64),
            resource: (!ask.resource.is_empty()).then(|| clip(ask.resource, 256)),
            sdk: Some(crate::agent::label(ask.user_agent))
                .filter(|s| !s.is_empty())
                .map(Into::into),
        });
    }
    if !caller.is_empty() && !rule.callers.contains(&caller) {
        if rule.callers.len() < MOST_KEPT_CALLERS {
            rule.callers.push(caller);
        } else {
            rule.more_callers = rule.more_callers.saturating_add(1);
        }
    }
}
