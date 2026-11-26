//! A rule as the API sends it, and what a Fail rule answers each service with.

use super::lab_build::title;
use crate::facts::Facts;
use crate::lab::codes::ROWS;
use crate::lab::rule::Fail as Failure;
use crate::lab::{Effect, EndedWhy, Matcher, Rule, Share, words};
use crate::model::{
    LabCode, LabEffect, LabEnd, LabFailKind, LabFirst, LabKind, LabMatcher, LabRule, LabShare,
};

/// The code each service the matcher names answers a Fail rule with (every API of the build, when it names none).
pub(super) fn answers(m: &Matcher, fail: &Failure) -> Vec<LabCode> {
    ROWS.iter()
        .filter(|r| {
            m.services.is_empty()
                || m.services
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(r.signing) || s.eq_ignore_ascii_case(r.sdk_id))
        })
        .map(|r| {
            let (_, status, code) = r.answer(fail);
            LabCode {
                service: r.sdk_id.to_owned(),
                status,
                code: code.into_owned(),
            }
        })
        .collect()
}

fn matcher_view(m: &Matcher) -> LabMatcher {
    let strings = |l: &[Box<str>]| l.iter().map(ToString::to_string).collect();
    LabMatcher {
        services: strings(&m.services),
        operations: strings(&m.operations),
        regions: strings(&m.regions),
        callers: strings(&m.callers),
        resources: m.resources.iter().map(|p| p.as_str().to_owned()).collect(),
    }
}

/// What a rule does, as the API sends it.
fn effect_view(effect: &Effect) -> (LabKind, LabEffect) {
    match effect {
        Effect::Slow {
            delay_ms,
            jitter_ms,
        } => (
            LabKind::Slow,
            LabEffect::Slow {
                delay_ms: *delay_ms,
                jitter_ms: *jitter_ms,
            },
        ),
        Effect::Drop => (LabKind::Drop, LabEffect::Drop),
        Effect::Fail(f) => {
            let (error, status, code) = match f {
                Failure::Throttle => (LabFailKind::Throttle, None, None),
                Failure::Unavailable => (LabFailKind::Unavailable, None, None),
                Failure::Internal => (LabFailKind::Internal, None, None),
                Failure::AccessDenied => (LabFailKind::AccessDenied, None, None),
                Failure::Custom { status, code } => {
                    (LabFailKind::Custom, Some(*status), Some(code.to_string()))
                }
            };
            (
                LabKind::Fail,
                LabEffect::Fail {
                    error,
                    status,
                    code,
                },
            )
        }
    }
}

/// A rule as the API sends it; `over` is when and why it ended, for one that did.
#[must_use]
pub(super) fn rule_view(facts: &Facts, rule: &Rule, over: Option<(i64, EndedWhy)>) -> LabRule {
    let spec = &rule.spec;
    let (kind, effect) = effect_view(&spec.effect);
    let share = match spec.share {
        Share::Every => LabShare::Every,
        Share::Nth(n) => LabShare::Nth { n },
        Share::Percent(percent) => LabShare::Percent { percent },
    };
    let title_of = |w: &str| title(facts, w);
    LabRule {
        id: rule.id,
        name: rule.name.to_string(),
        kind,
        sentence: words::sentence(spec, &title_of),
        enabled: rule.enabled,
        matched: rule.matched,
        hits: rule.hits,
        started_ms: rule.created_ms,
        ends_ms: over.map_or(rule.ends_ms, |(at, _)| at),
        last_hit_ms: rule.last_ms,
        matcher: matcher_view(&spec.matcher),
        effect,
        share,
        max_hits: spec.max_hits,
        what: words::what(spec, &title_of),
        target: words::target(spec),
        share_words: words::share_words(spec.share),
        answers: match &spec.effect {
            Effect::Fail(f) => answers(&spec.matcher, f),
            _ => Vec::new(),
        },
        first: rule.first.as_ref().map(|f| LabFirst {
            at_ms: f.at_ms,
            caller: f.caller.to_string(),
            service: f.service.to_owned(),
            operation: f.operation.to_string(),
            resource: f.resource.as_ref().map(ToString::to_string),
            sdk: f.sdk.as_ref().map(ToString::to_string),
        }),
        callers: rule.callers.iter().map(ToString::to_string).collect(),
        more_callers: rule.more_callers,
        ended: over.map(|(_, why)| match why {
            EndedWhy::Expired => LabEnd::Expired,
            EndedWhy::Spent => LabEnd::Spent,
            EndedWhy::Cleared => LabEnd::Cleared,
            EndedWhy::Replaced => LabEnd::Replaced,
        }),
    }
}
