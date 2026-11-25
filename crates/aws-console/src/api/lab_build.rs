//! The Lab API's translations: a rule as the form sends it into a [`Spec`] (with every problem in it said), and a rule of the table
//! into the view the API sends. The names a person types (`stepfunctions`, `SQS`, `DynamoDB Streams`) are read here, against the
//! services of this build; the table itself only compares words.

use aws_wire::service::Service;

use crate::facts::Facts;
use crate::lab::codes::ROWS;
use crate::lab::rule::Fail as Failure;
use crate::lab::{
    AddError, Effect, EndedWhy, LONGEST_CODE, LONGEST_DELAY_MS, LONGEST_NAME, LONGEST_PATTERN,
    LONGEST_RULE, MOST_CALLERS, MOST_ENDED, MOST_KEPT_CALLERS, MOST_OPERATIONS, MOST_REGIONS,
    MOST_RESOURCES, MOST_RULES, MOST_SERVICES, Matcher, NTH, PERCENT, Rule, SHORTEST_RULE, Share,
    Spec, words,
};
use crate::model::{
    LabCheck, LabCode, LabEffect, LabEnd, LabFailKind, LabFirst, LabKind, LabLimits, LabMatcher,
    LabProblem, LabRule, LabRuleBody, LabShare,
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
fn resolve(facts: &Facts, word: &str) -> Option<Vec<String>> {
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

fn effect_of(e: &LabEffect, problems: &mut Vec<AddError>) -> Effect {
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
            LabFailKind::Custom => {
                if status.is_none() {
                    problems.push(AddError::BadStatus(0));
                }
                Failure::Custom {
                    status: status.unwrap_or(0),
                    code: code.clone().unwrap_or_default().trim().into(),
                }
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
    let effect = effect_of(&body.effect, &mut problems);
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
    for p in spec.problems() {
        if !problems.contains(&p) {
            problems.push(p);
        }
    }
    (spec, problems)
}

/// The code each service the matcher names answers a Fail rule with (every API of the build, when it names none).
fn answers(m: &Matcher, fail: &Failure) -> Vec<LabCode> {
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
