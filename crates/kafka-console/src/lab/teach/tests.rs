use super::*;

fn every_text(t: &LabTeach) -> Vec<&str> {
    let mut all = vec![&*t.title, &*t.lede, &*t.simulates, &*t.expect, &*t.watch];
    all.extend(t.client.iter().map(String::as_str));
    for set in &t.settings {
        all.extend([&*set.name, &*set.side, &*set.default, &*set.why]);
    }
    all
}

#[test]
fn every_rehearsal_is_taught_once_in_the_order_of_the_tabs() {
    let r = rehearsals();
    let tabs: Vec<&str> = r.iter().map(|t| t.tab.as_str()).collect();
    assert_eq!(tabs, TABS);
    for t in &r {
        assert!(
            !t.client.is_empty() && t.client.len() <= 4,
            "{}: 1 to 4 points",
            t.tab
        );
        assert!(
            (3..=5).contains(&t.settings.len()),
            "{}: 3 to 5 settings",
            t.tab
        );
        for set in &t.settings {
            assert!(
                ["producer", "consumer", "client", "topic"].contains(&set.side.as_str()),
                "{}",
                set.name
            );
        }
    }
}

#[test]
fn the_teaching_is_short_plain_sentences() {
    let intro = intro();
    let mut all: Vec<String> = vec![intro.what.clone()];
    all.extend(intro.limits.iter().cloned());
    all.extend(
        intro
            .steps
            .iter()
            .flat_map(|s| [s.title.clone(), s.text.clone()]),
    );
    for t in rehearsals() {
        all.extend(every_text(&t).into_iter().map(str::to_owned));
        for line in [&t.lede, &t.simulates, &t.expect, &t.watch] {
            assert!(line.ends_with('.'), "a sentence: {line}");
            assert!(line.chars().count() <= 240, "short: {line}");
        }
    }
    assert_eq!(intro.steps.len(), 3);
    for text in all {
        // A line continued in the source never leaves a newline or a run of spaces in the words.
        assert!(!text.contains('\n') && !text.contains("  "), "{text:?}");
        assert!(!text.is_empty());
    }
}

#[test]
fn the_defaults_are_kafkas() {
    // The Java client's and the broker's defaults (Kafka 4): a regression here is a wrong lesson.
    let want = [
        ("request.timeout.ms", "30000"),
        ("delivery.timeout.ms", "120000"),
        ("max.block.ms", "60000"),
        ("max.poll.interval.ms", "300000"),
        ("retries", "2147483647"),
        ("retry.backoff.ms", "100"),
        ("retry.backoff.max.ms", "1000"),
        ("enable.idempotence", "true"),
        ("max.in.flight.requests.per.connection", "5"),
        ("reconnect.backoff.ms", "50"),
        ("reconnect.backoff.max.ms", "1000"),
        ("session.timeout.ms", "45000"),
        ("heartbeat.interval.ms", "3000"),
        ("auto.commit.interval.ms", "5000"),
        ("min.cleanable.dirty.ratio", "0.5"),
        ("delete.retention.ms", "86400000"),
    ];
    let all: Vec<LabSetting> = rehearsals().into_iter().flat_map(|t| t.settings).collect();
    for (name, default) in want {
        let found: Vec<&LabSetting> = all.iter().filter(|s| s.name == name).collect();
        assert!(!found.is_empty(), "{name} is taught");
        for s in found {
            assert_eq!(s.default, default, "{name}");
        }
    }
}
