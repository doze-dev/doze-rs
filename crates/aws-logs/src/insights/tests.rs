#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::parse;
use super::record::Event;
use super::run::Engine;

/// Runs `query` over `messages` (newest last; fed newest first, as the reader does), one a second from t=1,700,000,000,000.
fn run(query: &str, messages: &[&str]) -> Vec<Vec<(String, String)>> {
    let q = parse(query).unwrap_or_else(|e| panic!("{query}: {}", e.message));
    let mut engine = Engine::new(&q, 10_000);
    for (i, m) in messages.iter().enumerate().rev() {
        let e = Event {
            ts: 1_700_000_000_000 + i64::try_from(i).unwrap() * 1000,
            ingested: 1_700_000_000_000,
            message: m,
            transformed: None,
            stream: if i % 2 == 0 { "a" } else { "b" },
            group: "/app",
            account: "000000000000",
            ptr: format!("p{i}"),
            lambda: false,
        };
        engine.push(&e);
        if engine.done() {
            break;
        }
    }
    let out = engine.finish();
    assert!(out.failed.is_none(), "{:?}", out.failed);
    out.rows
}

fn column(rows: &[Vec<(String, String)>], name: &str) -> Vec<String> {
    rows.iter()
        .map(|r| {
            r.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        })
        .collect()
}

const LINES: [&str; 5] = [
    r#"{"level":"info","ms":10,"path":"/a"}"#,
    r#"{"level":"error","ms":250,"path":"/b"}"#,
    r#"{"level":"info","ms":30,"path":"/a"}"#,
    "plain ERROR line user=bob action=login",
    r#"{"level":"error","ms":40,"path":"/a"}"#,
];

#[test]
fn a_query_with_no_fields_shows_time_message_and_pointer_newest_first() {
    let rows = run("limit 2", &LINES);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0][0],
        ("@timestamp".into(), "2023-11-14 22:13:24.000".into())
    );
    assert_eq!(rows[0][1].0, "@message");
    assert_eq!(rows[0][2], ("@ptr".into(), "p4".into()));
}

#[test]
fn filters_compare_json_fields_and_match_patterns() {
    let rows = run(
        "fields path, ms | filter level = 'error' and ms > 100",
        &LINES,
    );
    assert_eq!(column(&rows, "path"), ["/b"]);
    let rows = run("filter @message like /ERROR/ | fields @message", &LINES);
    assert_eq!(rows.len(), 1);
    let rows = run("filter @message like 'info' | fields @logStream", &LINES);
    assert_eq!(column(&rows, "@logStream"), ["a", "a"]);
    let rows = run("filter path in ['/b'] or ms = 10 | fields ms", &LINES);
    assert_eq!(column(&rows, "ms"), ["250", "10"]);
    let rows = run("filter not ispresent(level) | fields @message", &LINES);
    assert_eq!(rows.len(), 1);
}

#[test]
fn stats_group_count_and_average() {
    let rows = run("stats count(*) as n, avg(ms), max(ms) by level", &LINES);
    // The line with no level is a group of its own, shown without the field.
    assert_eq!(column(&rows, "level"), ["", "error", "info"]);
    assert_eq!(column(&rows, "n"), ["1", "2", "2"]);
    assert_eq!(column(&rows, "avg(ms)"), ["", "145", "20"]);
    assert_eq!(column(&rows, "max(ms)"), ["", "250", "30"]);
    let rows = run("stats count() by bin(1h)", &LINES);
    assert_eq!(
        rows[0],
        [
            ("bin(1h)".into(), "2023-11-14 22:00:00.000".into()),
            ("count()".into(), "5".into())
        ]
    );
    let rows = run(
        "stats sum(ms) / count(ms) as mean, pct(ms, 50) as p50",
        &LINES,
    );
    assert_eq!(column(&rows, "mean"), ["82.5"]);
    assert_eq!(column(&rows, "p50"), ["30"]);
    let rows = run(
        "stats earliest(path) as first, latest(path) as last, count_distinct(path) as d",
        &LINES,
    );
    assert_eq!(
        rows[0],
        [
            ("first".into(), "/a".into()),
            ("last".into(), "/a".into()),
            ("d".into(), "2".into())
        ]
    );
}

#[test]
fn commands_after_stats_run_over_the_groups() {
    let rows = run(
        "stats count(*) as n by path | filter n > 1 | sort n desc",
        &LINES,
    );
    assert_eq!(column(&rows, "path"), ["/a"]);
    let rows = run("fields ms | sort ms desc | limit 2", &LINES);
    assert_eq!(column(&rows, "ms"), ["250", "40"]);
}

#[test]
fn parse_takes_fields_out_with_a_glob_or_a_regular_expression() {
    let rows = run(
        "parse @message 'user=* action=*' as user, action | filter ispresent(user) | display user, action",
        &LINES,
    );
    assert_eq!(rows[0][0], ("user".into(), "bob".into()));
    assert_eq!(rows[0][1], ("action".into(), "login".into()));
    // Recorded 2026-11-19: a bare `fields` naming a field `parse` made is refused.
    assert_eq!(
        super::parse("parse @message 'user=* action=*' as user, action | fields user")
            .unwrap_err()
            .message,
        "Ephemeral field is already defined: user"
    );
    let rows = run(
        r"parse @message /user=(?<who>\w+)/ | filter ispresent(who)",
        &LINES,
    );
    assert_eq!(column(&rows, "who"), ["bob"]);
}

#[test]
fn functions_work_on_strings_numbers_and_times() {
    let rows = run(
        "fields toupper(level) as up, strlen(path) as n, concat(path, '!') as c, abs(-3) as a, \
         substr(level, 0, 3) as s, replace(path, '/', '_') as r, datefloor(@timestamp, 1m) as t | limit 1",
        &LINES,
    );
    let r = &rows[0];
    let get = |k: &str| r.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("up"), Some("ERROR"));
    assert_eq!(get("n"), Some("2"));
    assert_eq!(get("c"), Some("/a!"));
    assert_eq!(get("a"), Some("3"));
    assert_eq!(get("s"), Some("err"));
    assert_eq!(get("r"), Some("_a"));
    assert_eq!(get("t"), Some("2023-11-14 22:13:00.000"));
    let rows = run(
        "fields isIpv4InSubnet('10.1.2.3', '10.0.0.0/8') as inside | limit 1",
        &LINES,
    );
    assert_eq!(column(&rows, "inside"), ["true"]);
}

#[test]
fn dedup_keeps_the_first_of_each() {
    let rows = run("fields path | dedup path", &LINES);
    // An event without the field is never a duplicate.
    assert_eq!(column(&rows, "path"), ["/a", "", "/b"]);
}

#[test]
fn malformed_queries_are_refused_with_a_place() {
    for bad in [
        "fields",
        "stats",
        "filter (a = 1",
        "nosuch x",
        "fields foo(1)",
        "stats sum(count(x))",
        "stats x by y",
        "limit 0",
        "parse @message 'a*b*' as one",
        "filter a = 'unterminated",
        "filter a like /[/",
        "fields a | | fields b",
        "pattern @message",
    ] {
        let e = parse(bad).expect_err(bad);
        assert!(
            e.message.contains("at line 1 and position"),
            "{bad}: {}",
            e.message
        );
    }
    assert!(parse("fields a # a comment\n| limit 1").is_ok());
    assert!(parse("").is_ok());
    let deep = format!("filter {}1{}", "(".repeat(500), ")".repeat(500));
    assert!(parse(&deep).is_err());
}
