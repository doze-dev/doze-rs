//! Logs Insights through the operations: a query is started, run (in line under test), read back, described and stopped.

use serde_json::{Value, json};

use super::{NOW, World};

fn world() -> World {
    let mut w = World::new();
    w.group("/app");
    w.stream("/app", "a");
    w.stream("/app", "b");
    w.put(
        "/app",
        "a",
        NOW - 3000,
        &[
            r#"{"level":"info","ms":10}"#,
            r#"{"level":"error","ms":90}"#,
        ],
    );
    w.put(
        "/app",
        "b",
        NOW - 1000,
        &[r#"{"level":"info","ms":30}"#, "plain line"],
    );
    w
}

fn start(w: &mut World, query: &str) -> String {
    let started = w.ok(
        "StartQuery",
        &json!({"logGroupName": "/app", "startTime": NOW / 1000 - 60, "endTime": NOW / 1000, "queryString": query}),
    );
    started["queryId"].as_str().unwrap().to_owned()
}

fn rows(v: &Value) -> Vec<Vec<(String, String)>> {
    v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            r.as_array()
                .unwrap()
                .iter()
                .map(|f| {
                    (
                        f["field"].as_str().unwrap().to_owned(),
                        f["value"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_query_runs_over_every_stream_newest_first_and_points_at_each_event() {
    let mut w = world();
    let id = start(&mut w, "fields @timestamp, @message, @logStream | limit 3");
    let got = w.ok("GetQueryResults", &json!({"queryId": id}));
    assert_eq!(got["status"], "Complete");
    let r = rows(&got);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0][1], ("@message".into(), "plain line".into()));
    assert_eq!(r[0][2], ("@logStream".into(), "b".into()));
    assert_eq!(r[0][3].0, "@ptr");
    assert_eq!(got["statistics"]["recordsMatched"], 3.0);
    assert_eq!(got["statistics"]["logGroupsScanned"], 1.0);
    // The pointer gives the whole event back, its times as milliseconds.
    let record = w.ok("GetLogRecord", &json!({"logRecordPointer": r[1][3].1}));
    assert_eq!(record["logRecord"]["level"], "info");
    assert_eq!(record["logRecord"]["ms"], "30");
    assert_eq!(record["logRecord"]["@logStream"], "b");
    assert_eq!(record["logRecord"]["@timestamp"], (NOW - 1000).to_string());
    a_record_carries_what_aws_adds(&mut w, &record);
}

/// Recorded 2026-11-19: the fields every ordinary event carries, the ids of its group and stream, and a pointer refused.
fn a_record_carries_what_aws_adds(w: &mut World, record: &Value) {
    assert_eq!(record["logRecord"]["@aws.account"], "000000000000");
    assert_eq!(record["logRecord"]["@aws.region"], "us-east-1");
    assert_eq!(record["logRecord"]["@data_format"], "Default");
    assert_eq!(record["logRecord"]["@data_source_name"], "Unknown");
    assert_eq!(record["logRecord"]["@data_source_type"], "Unknown");
    let group_id = record["logRecord"]["@logGroupId"].as_str().unwrap();
    assert_eq!(group_id.len(), 36);
    let stream_id = record["logRecord"]["@logStreamId"].as_str().unwrap();
    let parts: Vec<&str> = stream_id.split("::").collect();
    assert_eq!(parts[0], group_id);
    // The SHA-256 of `b`.
    assert_eq!(
        parts[1],
        "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d"
    );
    let (code, message) = w.err(
        "GetLogRecord",
        &json!({"logRecordPointer": "bm90IGEgcG9pbnRlcg=="}),
    );
    assert_eq!(code, "InvalidParameterException");
    assert_eq!(message, "InvalidParameter Unable to deserialize token");
}

#[test]
fn stats_queries_aggregate_and_pages_continue() {
    let mut w = world();
    let id = start(
        &mut w,
        "filter ispresent(level) | stats count(*) as n, avg(ms) as mean by level",
    );
    let got = w.ok("GetQueryResults", &json!({"queryId": id}));
    assert_eq!(
        rows(&got),
        [
            vec![
                ("level".into(), "error".into()),
                ("n".into(), "1".into()),
                ("mean".into(), "90".into())
            ],
            vec![
                ("level".into(), "info".into()),
                ("n".into(), "2".into()),
                ("mean".into(), "20".into())
            ],
        ]
    );
    let id = start(&mut w, "fields @message");
    let first = w.ok("GetQueryResults", &json!({"queryId": id, "maxItems": 3}));
    assert_eq!(rows(&first).len(), 3);
    let token = first["nextToken"].as_str().unwrap();
    let rest = w.ok(
        "GetQueryResults",
        &json!({"queryId": id, "maxItems": 3, "nextToken": token}),
    );
    assert_eq!(rows(&rest).len(), 1);
    assert!(rest.get("nextToken").is_none());
}

#[test]
fn queries_are_described_listed_and_refused_to_stop_once_ended() {
    let mut w = world();
    let id = start(&mut w, "fields @message");
    let all = w.ok("DescribeQueries", &json!({"logGroupName": "/app"}));
    assert_eq!(all["queries"][0]["queryId"], id.as_str());
    assert_eq!(all["queries"][0]["status"], "Complete");
    // Recorded 2026-11-19: the query as AWS rewrote it, behind its groups and range.
    let shown = all["queries"][0]["queryString"].as_str().unwrap();
    assert!(
        shown.starts_with("SOURCE \"/app\" START=") && shown.ends_with(" | fields @message"),
        "{shown}"
    );
    let none = w.ok("DescribeQueries", &json!({"status": "Running"}));
    assert_eq!(none["queries"].as_array().unwrap().len(), 0);
    // The groups as the query named them.
    let groups = w.ok("ListLogGroupsForQuery", &json!({"queryId": id}));
    assert_eq!(groups["logGroupIdentifiers"][0], "/app");
    let (code, message) = w.err("StopQuery", &json!({"queryId": id}));
    assert_eq!(code, "InvalidParameterException");
    assert!(
        message.starts_with("Query is already ended with Complete (Service: AWSLogs;"),
        "{message}"
    );
    let (code, message) = w.err("GetQueryResults", &json!({"queryId": "no-such-query"}));
    assert_eq!(code, "InvalidParameterException");
    assert_eq!(message, "The provided queryId = no-such-query is invalid ");
}

#[test]
fn a_query_is_refused_for_what_it_names_and_how_it_is_written() {
    let mut w = world();
    let base = json!({"startTime": 1, "endTime": 2, "queryString": "fields @message"});
    let with = |extra: Value| {
        let mut v = base.clone();
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    };
    // Recorded 2026-11-19: with no group named, the query is read as one that must name its own.
    let (code, message) = w.err("StartQuery", &base);
    assert_eq!(code, "MalformedQueryException");
    assert!(
        message.starts_with("Invalid syntax while using query definition snippets: unexpected symbol found fields at line 1 and position 0 (Service: AWSLogs; Status Code: 400; Error Code: MalformedQueryException; Request ID: "),
        "{message}"
    );
    let both = with(json!({"logGroupName": "/app", "logGroupNames": ["/app"]}));
    assert_eq!(
        w.err("StartQuery", &both),
        (
            "InvalidParameterException".into(),
            "Both logGroupName and logGroupNames cannot be specified at the same time".into()
        )
    );
    let absent = with(json!({"logGroupNames": ["/absent"]}));
    assert_eq!(w.err("StartQuery", &absent).0, "ResourceNotFoundException");
    // Positions count from 0 in the rewritten query: `SOURCE "/app" START=1000 END=2999 | ` is 36 characters.
    let bad = with(json!({"logGroupName": "/app", "queryString": "fields | |"}));
    let (code, message) = w.err("StartQuery", &bad);
    assert_eq!(code, "MalformedQueryException");
    assert!(
        message
            .starts_with("unexpected symbol found | at line 1 and position 43 (Service: AWSLogs;"),
        "{message}"
    );
    let backwards = with(json!({"logGroupName": "/app", "startTime": 5, "endTime": 2}));
    let (code, message) = w.err("StartQuery", &backwards);
    assert_eq!(code, "InvalidParameterException");
    assert!(
        message.starts_with("End time cannot be less than Start time (Service: AWSLogs;"),
        "{message}"
    );
    let sql = with(json!({"logGroupName": "/app", "queryLanguage": "SQL"}));
    assert_eq!(w.err("StartQuery", &sql).0, "InvalidParameterException");
}

#[test]
fn group_fields_are_counted_over_the_recent_window() {
    let mut w = world();
    let got = w.ok("GetLogGroupFields", &json!({"logGroupName": "/app"}));
    let fields: Vec<(String, i64)> = got["logGroupFields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap().to_owned(),
                f["percent"].as_i64().unwrap(),
            )
        })
        .collect();
    assert!(fields.contains(&("@message".into(), 100)));
    assert!(fields.contains(&("level".into(), 75)));
    assert!(fields.contains(&("@aws.region".into(), 100)));
    // Recorded 2026-11-19: neither `@ptr` nor `@log` is listed.
    assert!(!fields.iter().any(|(f, _)| f == "@ptr" || f == "@log"));
}

#[test]
fn a_transformer_shapes_what_insights_reads() {
    let mut w = world();
    w.ok(
        "PutTransformer",
        &json!({"logGroupIdentifier": "/app", "transformerConfig": [
            {"grok": {"match": "%{WORD:verb} %{WORD:noun}"}},
            {"upperCaseString": {"withKeys": ["verb"]}},
        ]}),
    );
    w.put("/app", "a", NOW - 500, &["make tea"]);
    let id = start(&mut w, "filter verb = 'MAKE' | fields noun, @message");
    let got = w.ok("GetQueryResults", &json!({"queryId": id}));
    let r = rows(&got);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0][0], ("noun".into(), "tea".into()));
    // The original is what the stream gives back.
    assert_eq!(r[0][1], ("@message".into(), "make tea".into()));
}
