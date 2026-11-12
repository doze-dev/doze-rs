use super::*;

fn help(name: &'static str, default: &str, doc: &'static str) -> ConfigHelp {
    ConfigHelp {
        name,
        default: default.to_owned(),
        doc,
    }
}

#[test]
fn kafkas_documentation_becomes_plain_text_of_a_few_sentences() {
    let doc = "This config designates the retention policy to use on log segments. The \"delete\" policy (which is the default) will discard old segments. The \"compact\" policy will enable <a href=\"#compaction\">log compaction</a>, which retains the latest value for each key. It is also possible to specify both policies in a comma-separated list (e.g. \"delete,compact\"). In this case, old segments will be discarded per the retention time and size configuration, while retained segments will be compacted. An empty list means infinite retention - no cleanup policies will be applied and log segments will be retained indefinitely.";
    let plain = plain_doc(doc);
    assert!(!plain.contains('<') && !plain.contains("href"), "{plain}");
    assert!(
        plain.starts_with("This config designates the retention policy to use on log segments."),
        "{plain}"
    );
    assert!(
        plain.contains("log compaction, which retains the latest value for each key."),
        "{plain}"
    );
    assert!(
        plain.chars().count() <= 420,
        "{} chars",
        plain.chars().count()
    );
    assert!(plain.ends_with('.'), "{plain}");
}

#[test]
fn plain_text_has_no_tags_whatever_the_input() {
    for (raw, want) in [
        ("", ""),
        ("no tags", "no tags"),
        ("a <code>b</code> c", "a b c"),
        ("a\n\n  b", "a b"),
        ("unclosed <tag", "unclosed"),
        ("a > b", "a > b"),
        ("<script>x</script>y", "xy"),
        ("one. two. three", "one. two. three"),
    ] {
        assert_eq!(plain_doc(raw), want, "{raw:?}");
    }
    // A single very long sentence is kept whole rather than cut mid-word.
    let long = "word ".repeat(200);
    assert_eq!(plain_doc(&long), long.trim_end());
}

#[test]
fn the_name_teaching_says_whether_the_name_is_free() {
    let free = name(Some(&Taken::Free));
    let text = format!("{:?}", free.here);
    assert!(text.contains("The name is free."), "{text}");
    assert!(format!("{:?}", name(Some(&Taken::Exists)).here).contains("has this name already"));
    let c = name(Some(&Taken::Collides("orders.dlq".to_owned())));
    assert!(c.here.contains(&Span::mono("orders.dlq")));
    assert!(!format!("{:?}", name(None).here).contains("free"));
    assert!(free.says.contains("249"));
}

#[test]
fn the_partition_teaching_uses_the_brokers_limits_when_it_has_them() {
    let l = Limits {
        max_partitions: 1000,
        default_partitions: 1,
    };
    let with = partitions(Some(l));
    assert_eq!(with.range[0], Span::text("1 to 1,000"));
    assert!(with.range.contains(&Span::mono("--max-partitions")));
    assert_eq!(with.default[0], Span::mono("1"));
    let without = partitions(None);
    assert_eq!(without.range, vec![Span::text("1 or more")]);
    assert_eq!(
        partitions(Some(Limits {
            max_partitions: 50,
            default_partitions: 6
        }))
        .default[0],
        Span::mono("6")
    );
}

#[test]
fn the_config_teaching_comes_from_the_table_it_is_given() {
    let c = cleanup(&help(
        "cleanup.policy",
        "delete",
        "This config designates the retention policy. More.",
    ));
    assert_eq!(c.says, "This config designates the retention policy. More.");
    assert_eq!(c.default, vec![Span::mono("delete")]);
    let r = retention(&help("retention.ms", "604800000", "The maximum time."));
    assert_eq!(
        r.default,
        vec![Span::mono("604800000"), Span::text(" (7 d)")]
    );
    let never = retention(&help("retention.ms", "-1", "x"));
    assert_eq!(
        never.default,
        vec![Span::mono("-1"), Span::text(" (no limit)")]
    );
    let odd = retention(&help("retention.ms", "soon", "x"));
    assert_eq!(odd.default, vec![Span::mono("soon")]);
}
