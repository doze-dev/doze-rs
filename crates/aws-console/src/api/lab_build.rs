//! The Lab API's translations: a rule as the form sends it into a [`Spec`] (with every problem in it said), and a rule of the table
//! into the view the API sends. The names a person types (`stepfunctions`, `SQS`, `DynamoDB Streams`) are read here, against the
//! services of this build; the table itself only compares words.

use aws_wire::service::Service;

use super::lab_custom::custom;
use super::lab_view::answers;
use crate::facts::Facts;
use crate::lab::rule::Fail as Failure;
use crate::lab::{
    AddError, Effect, LONGEST_CODE, LONGEST_DELAY_MS, LONGEST_NAME, LONGEST_PATTERN, LONGEST_RULE,
    MOST_CALLERS, MOST_ENDED, MOST_KEPT_CALLERS, MOST_OPERATIONS, MOST_REGIONS, MOST_RESOURCES,
    MOST_RULES, MOST_SERVICES, Matcher, NTH, PERCENT, SHORTEST_RULE, Share, Spec, words,
};
use crate::model::{
    LabCheck, LabEffect, LabFailKind, LabLimits, LabMatcher, LabProblem, LabRuleBody, LabShare,
};

/// What the Lab is bounded by.
#[must_use]
pub(super) fn limits() -> LabLimits {
    let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    LabLimits {
        most_rules: n(MOST_RULES),
        most_ended: n(MOST_ENDED),
        shortest_secs: n(usize::try_from(SHORTEST_RULE.as_secs()).unwrap_or(1)),
        longest_secs: n(usize::try_from(LONGEST_RULE.as_secs()).unwrap_or(86_400)),
        longest_delay_ms: LONGEST_DELAY_MS,
        least_nth: *NTH.start(),
        most_nth: *NTH.end(),
        least_percent: u32::from(*PERCENT.start()),
        most_percent: u32::from(*PERCENT.end()),
        most_services: n(MOST_SERVICES),
        most_operations: n(MOST_OPERATIONS),
        most_regions: n(MOST_REGIONS),
        most_callers: n(MOST_CALLERS),
        most_resources: n(MOST_RESOURCES),
        longest_name: n(LONGEST_NAME),
        longest_pattern: n(LONGEST_PATTERN),
        longest_code: n(LONGEST_CODE),
        callers_kept: n(MOST_KEPT_CALLERS),
    }
}

fn apis(facts: &Facts) -> impl Iterator<Item = &dyn Service> {
    facts
        .services
        .iter()
        .flat_map(|s| s.apis.iter())
        .map(|a| &**a)
}

/// What the words a person typed for a service mean: the signing names of its APIs when it is a catalogue name (`stepfunctions` is
/// `states`), the word as it is when it is a signing name or an SDK id, and `None` when this build has no such service.
pub(super) fn resolve(facts: &Facts, word: &str) -> Option<Vec<String>> {
    if let Some(s) = facts
        .services
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(word))
    {
        let mut names: Vec<String> = s
            .apis
            .iter()
            .map(|a| a.meta().signing_name.to_owned())
            .collect();
        names.sort();
        names.dedup();
        return Some(if names.is_empty() {
            vec![word.to_owned()]
        } else {
            names
        });
    }
    apis(facts)
        .any(|a| {
            a.meta().signing_name.eq_ignore_ascii_case(word)
                || a.meta().sdk_id.eq_ignore_ascii_case(word)
        })
        .then(|| vec![word.to_owned()])
}

/// The APIs of the build that `services` (signing names or SDK ids) name, with their models.
pub(super) fn models_fitting(
    facts: &Facts,
    services: &[impl AsRef<str>],
) -> Vec<crate::lab::listed::Model> {
    apis(facts)
        .filter(|a| {
            services.iter().any(|s| {
                s.as_ref().eq_ignore_ascii_case(a.meta().signing_name)
                    || s.as_ref().eq_ignore_ascii_case(a.meta().sdk_id)
            })
        })
        .map(|a| (a.meta(), a.tables()))
        .collect()
}

/// The service a word of a rule's matcher is, as a person reads it (`states` is `Step Functions`; an SDK id is itself).
#[must_use]
pub(super) fn title(facts: &Facts, word: &str) -> String {
    if let Some(api) = apis(facts).find(|a| a.meta().sdk_id.eq_ignore_ascii_case(word)) {
        return api.meta().sdk_id.to_owned();
    }
    facts
        .services
        .iter()
        .find(|s| {
            s.apis
                .iter()
                .any(|a| a.meta().signing_name.eq_ignore_ascii_case(word))
        })
        .map_or_else(|| word.to_owned(), |s| s.title.to_owned())
}

/// Whether an API of the build that one of `services` names (all of them when none are named) has operation `op`.
fn has_operation(facts: &Facts, services: &[Box<str>], op: &str) -> bool {
    apis(facts)
        .filter(|a| {
            services.is_empty()
                || services.iter().any(|s| {
                    s.eq_ignore_ascii_case(a.meta().signing_name)
                        || s.eq_ignore_ascii_case(a.meta().sdk_id)
                })
        })
        .any(|a| a.tables().find_op(op).is_some())
}

fn matcher_of(facts: &Facts, m: &LabMatcher, problems: &mut Vec<AddError>) -> Matcher {
    let mut services = Vec::new();
    let mut all_known = true;
    for word in &m.services {
        match resolve(facts, word.trim()) {
            Some(names) => services.extend(names),
            // An entry that is not a word at all (empty, a control character) is the matcher's to say.
            None if word.trim().is_empty() || word.chars().any(char::is_control) => {
                services.push(word.clone());
            }
            None => {
                all_known = false;
                problems.push(AddError::UnknownService(word.trim().to_owned()));
            }
        }
    }
    let (matcher, said) = Matcher::build(
        &services,
        &m.operations,
        &m.regions,
        &m.callers,
        &m.resources,
    );
    problems.extend(said);
    if all_known {
        for op in &matcher.operations {
            if !has_operation(facts, &matcher.services, op) {
                problems.push(AddError::UnknownOperation(op.to_string()));
            }
        }
    }
    matcher
}

fn effect_of(
    facts: &Facts,
    e: &LabEffect,
    matcher: &Matcher,
    problems: &mut Vec<AddError>,
) -> Effect {
    match e {
        LabEffect::Slow {
            delay_ms,
            jitter_ms,
        } => Effect::Slow {
            delay_ms: *delay_ms,
            jitter_ms: *jitter_ms,
        },
        LabEffect::Drop => Effect::Drop,
        LabEffect::Fail {
            error,
            status,
            code,
        } => Effect::Fail(match error {
            LabFailKind::Throttle => Failure::Throttle,
            LabFailKind::Unavailable => Failure::Unavailable,
            LabFailKind::Internal => Failure::Internal,
            LabFailKind::AccessDenied => Failure::AccessDenied,
            // What the service or an operation could not be read as is said already, and the model is not asked about it.
            LabFailKind::Custom => {
                let unread = problems
                    .iter()
                    .any(|p| matches!(p.field(), "services" | "operations"));
                let mut said = Vec::new();
                let fail = custom(
                    facts,
                    &matcher.services,
                    &matcher.operations,
                    *status,
                    code.as_deref().unwrap_or_default(),
                    &mut said,
                );
                if !unread {
                    problems.extend(said);
                }
                fail
            }
        }),
    }
}

/// The rule the form asks for, and everything wrong with it. The rule is made of what could be read, so a sentence can be said of it
/// even when it cannot be made.
#[must_use]
pub(super) fn build(facts: &Facts, body: &LabRuleBody) -> (Spec, Vec<AddError>) {
    let mut problems = Vec::new();
    let matcher = matcher_of(facts, &body.matcher, &mut problems);
    let effect = effect_of(facts, &body.effect, &matcher, &mut problems);
    let share = match body.share {
        LabShare::Every => Share::Every,
        LabShare::Nth { n } => Share::Nth(n),
        LabShare::Percent { percent } => Share::Percent(percent),
    };
    let spec = Spec {
        name: body.name.trim().to_owned(),
        matcher,
        effect,
        share,
        ttl: std::time::Duration::from_secs(body.ends_in_secs),
        max_hits: body.max_hits,
    };
    // A rule that cannot say which errors to choose from has no code to judge: the missing service or operation is what to fix.
    let needs = problems
        .iter()
        .any(|p| matches!(p, AddError::CustomNeeds(_)));
    for p in spec.problems() {
        if !problems.contains(&p) && !(needs && p == AddError::BadCode) {
            problems.push(p);
        }
    }
    (spec, problems)
}

/// What `POST /api/v1/lab/rules/check` answers: the sentence, and every problem in words.
#[must_use]
pub(super) fn check(facts: &Facts, body: &LabRuleBody, room: impl Fn(&Spec) -> bool) -> LabCheck {
    let (spec, mut problems) = build(facts, body);
    if problems.is_empty() && !room(&spec) {
        problems.push(AddError::Full);
    }
    LabCheck {
        ok: problems.is_empty(),
        sentence: words::sentence(&spec, &|w| title(facts, w)),
        problems: problems
            .iter()
            .map(|p| LabProblem {
                field: p.field().to_owned(),
                text: p.words(),
            })
            .collect(),
        answers: match &spec.effect {
            Effect::Fail(f) => answers(&spec.matcher, f),
            _ => Vec::new(),
        },
    }
}
