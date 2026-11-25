#![no_main]
//! Any bytes as the body of a rule: it is read, its words are made into a matcher, and the rule is checked, and none of it
//! panics; whatever is kept is within the Lab's bounds.

use libfuzzer_sys::fuzz_target;

use aws_console::lab::{Matcher, MOST_RESOURCES, MOST_SERVICES};
use aws_console::model::LabRuleBody;

fuzz_target!(|data: &[u8]| {
    let Ok(body) = serde_json::from_slice::<LabRuleBody>(data) else {
        return;
    };
    let m = &body.matcher;
    let (matcher, _said) = Matcher::build(&m.services, &m.operations, &m.regions, &m.callers, &m.resources);
    // A list over its bound is refused by the rule's check, never cut or panicked on.
    assert!(matcher.services.len() <= m.services.len().max(MOST_SERVICES));
    assert!(matcher.resources.len() <= m.resources.len().max(MOST_RESOURCES));
});
