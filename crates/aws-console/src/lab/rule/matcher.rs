//! Which calls a rule fits: the lists of words and patterns, tidied, and the test a call must pass.

use super::{AddError, Field, LONGEST_PATTERN, LONGEST_WORD};
use crate::lab::glob;

/// A pattern of a rule's resource list ([`glob`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pattern(Box<str>);

impl Pattern {
    /// A pattern, or `None` when it is empty, too long, or holds a control character.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let ok = !text.is_empty()
            && text.len() <= LONGEST_PATTERN
            && !text.chars().any(char::is_control);
        ok.then(|| Self(text.into()))
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the resource a call named fits: the whole text, or its last name.
    #[must_use]
    pub fn fits(&self, resource: &str) -> bool {
        glob::fits(&self.0, resource) || glob::fits(&self.0, glob::last_name(resource))
    }
}

/// Which calls a rule fits. Every list that is not empty must fit; an empty one fits anything. Each list is sorted and without
/// repeats, so two rules for the same calls are equal whatever order they were written in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matcher {
    /// Signing names, SDK ids (compared without regard to case).
    pub services: Vec<Box<str>>,
    /// Operations, exactly.
    pub operations: Vec<Box<str>>,
    /// Regions, exactly (lower case).
    pub regions: Vec<Box<str>>,
    /// Access key ids, exactly.
    pub callers: Vec<Box<str>>,
    /// What a call names.
    pub resources: Vec<Pattern>,
}

/// Sorts a list and drops what repeats.
fn tidy<T: Ord>(mut list: Vec<T>) -> Vec<T> {
    list.sort();
    list.dedup();
    list
}

fn word_ok(w: &str) -> bool {
    !w.is_empty() && w.len() <= LONGEST_WORD && !w.chars().any(char::is_control)
}

impl Matcher {
    // fuzz:parser lab_body
    /// A matcher from the words as typed. Each list is made tidy; an entry that is not one is left out and said (the matcher is then
    /// what the rest of the words mean, so a sentence can still be made of it).
    #[must_use]
    pub fn build(
        services: &[String],
        operations: &[String],
        regions: &[String],
        callers: &[String],
        resources: &[String],
    ) -> (Self, Vec<AddError>) {
        let mut problems = Vec::new();
        let mut words = |field: Field, list: &[String], lower: bool| -> Vec<Box<str>> {
            let mut kept = Vec::new();
            for w in list {
                let w = w.trim();
                if word_ok(w) {
                    kept.push(
                        if lower {
                            w.to_ascii_lowercase()
                        } else {
                            w.to_owned()
                        }
                        .into(),
                    );
                } else {
                    problems.push(AddError::BadEntry(field));
                }
            }
            kept
        };
        let services = words(Field::Services, services, true);
        let operations = words(Field::Operations, operations, false);
        let regions = words(Field::Regions, regions, true);
        let callers = words(Field::Callers, callers, false);
        let mut patterns = Vec::new();
        for r in resources {
            match Pattern::new(r.trim()) {
                Some(p) => patterns.push(p),
                None => problems.push(AddError::BadEntry(Field::Resources)),
            }
        }
        let matcher = Self {
            services: tidy(services),
            operations: tidy(operations),
            regions: tidy(regions),
            callers: tidy(callers),
            resources: tidy(patterns),
        };
        problems.extend(matcher.too_many());
        problems.dedup();
        (matcher, problems)
    }

    pub(super) fn too_many(&self) -> Vec<AddError> {
        [
            (Field::Services, self.services.len()),
            (Field::Operations, self.operations.len()),
            (Field::Regions, self.regions.len()),
            (Field::Callers, self.callers.len()),
            (Field::Resources, self.resources.len()),
        ]
        .into_iter()
        .filter(|(f, n)| *n > f.most())
        .map(|(f, _)| AddError::TooMany(f))
        .collect()
    }

    /// Whether every list that is set fits a call.
    #[must_use]
    pub fn fits(&self, ask: &aws_gateway::Ask<'_>) -> bool {
        let named =
            |list: &[Box<str>], what: &str| list.is_empty() || list.iter().any(|w| **w == *what);
        (self.services.is_empty()
            || self.services.iter().any(|s| {
                s.eq_ignore_ascii_case(ask.signing_name) || s.eq_ignore_ascii_case(ask.sdk_id)
            }))
            && named(&self.operations, ask.operation)
            && named(&self.regions, ask.region)
            && named(&self.callers, ask.access_key_id)
            && (self.resources.is_empty()
                || (!ask.resource.is_empty()
                    && self.resources.iter().any(|p| p.fits(ask.resource))))
    }
}
