#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn every_rehearsal_is_taught_once_in_the_order_of_the_tabs() {
    let tabs: Vec<String> = rehearsals().into_iter().map(|t| t.tab).collect();
    assert_eq!(
        tabs,
        [
            "throttle",
            "unavailable",
            "internal",
            "access-denied",
            "slow",
            "drop"
        ]
    );
}

#[test]
fn every_lesson_says_what_it_stands_for_what_to_expect_and_what_a_client_does() {
    for t in rehearsals() {
        for (what, text) in [

            ("lede", &t.lede),
            ("simulates", &t.simulates),
            ("expect", &t.expect),
            ("watch", &t.watch),
        ] {
            assert!(text.len() > 12, "{}: {what} is empty", t.tab);
        }
        assert!(t.title.len() > 3, "{}: its title", t.tab);
        assert!(t.client.len() >= 3, "{}: what a client does", t.tab);
        assert!(t.client.iter().all(|c| c.ends_with('.')));
        assert!(t.watch.contains("Live"), "{}: where to see it", t.tab);
    }
}

#[test]
fn the_retries_are_taught_with_the_defaults_the_sdks_ship_with() {
    let throttle = &rehearsals()[0];
    let by = |sdk: &str| {
        throttle
            .settings
            .iter()
            .filter(|s| s.sdk.contains(sdk))
            .map(|s| format!("{} = {}", s.name, s.default))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let boto = by("boto3");
    assert!(boto.contains("standard"), "{boto}");
    assert!(boto.contains("3 attempts"), "{boto}");
    assert!(boto.contains("4 for DynamoDB"), "{boto}");
    assert!(boto.contains("5 in legacy"), "{boto}");
    assert!(by("JavaScript").contains("maxAttempts"), "JS v3");
    assert!(by("JavaScript").contains("3, the first included"));
    assert!(by("Java").contains("2.58.0"));
    assert!(by("Go").contains("3 attempts"));
}

#[test]
fn the_timeouts_are_taught_with_slow_and_a_denial_is_taught_without_retries() {
    let all = rehearsals();
    let slow = all.iter().find(|t| t.tab == "slow").unwrap();
    let boto: Vec<&str> = slow
        .settings
        .iter()
        .filter(|s| s.sdk.contains("boto3"))
        .map(|s| s.default.as_str())
        .collect();
    assert_eq!(boto, ["60 s each"]);
    let denied = all.iter().find(|t| t.tab == "access-denied").unwrap();
    assert!(denied.settings.is_empty(), "no SDK retries a denial");
    assert!(denied.expect.contains("--iam enforce"));
}

#[test]
fn the_words_never_say_asleep_or_promise_what_is_not_true() {
    let g = guide();
    let mut text = format!("{} {}", g.intro.what, g.intro.limits.join(" "));
    for t in &g.rehearsals {
        text.push_str(&format!("{} {} {} {}", t.lede, t.simulates, t.expect, t.client.join(" ")));
    }
    assert!(!text.to_lowercase().contains("asleep"));
    // A fault never reaches storage, and the page and the doc say so in these words.
    assert!(text.contains("writes nothing"));
    assert_eq!(g.intro.steps.len(), 3);
}
