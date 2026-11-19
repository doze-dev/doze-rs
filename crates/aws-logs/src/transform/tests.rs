#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use serde_json::{Value, json};

use super::{Meta, Transformer};

const META: Meta<'static> = Meta {
    group: "/app",
    stream: "web-1",
    account: "111122223333",
    region: "eu-west-1",
};

fn run(config: &Value, message: &str) -> Option<Value> {
    let t = Transformer::parse(config).unwrap_or_else(|e| panic!("{config}: {}", e.message));
    t.apply(message, &META)
        .map(|s| serde_json::from_str(&s).unwrap())
}

fn refused(config: &Value) -> String {
    let e = Transformer::parse(config).unwrap_err();
    assert_eq!(e.shape.as_deref(), Some("InvalidParameterException"));
    e.message
}

/// What refusing `processor` says when it comes after a parser (a transformer must begin with one).
fn later(processor: Value) -> String {
    refused(&json!([{"parseJSON": {}}, processor]))
}

#[test]
fn parse_json_then_keys_are_added_copied_moved_renamed_and_deleted() {
    let config = json!([
        {"parseJSON": {}},
        {"addKeys": {"entries": [{"key": "env", "value": "dev"}, {"key": "level", "value": "x"}]}},
        {"copyValue": {"entries": [{"source": "@logGroupName", "target": "meta.group"}, {"source": "@regionName", "target": "meta.region"}]}},
        {"moveKeys": {"entries": [{"source": "user.id", "target": "meta"}]}},
        {"renameKeys": {"entries": [{"key": "msg", "renameTo": "text"}]}},
        {"deleteKeys": {"withKeys": ["drop"]}},
    ]);
    let out = run(
        &config,
        r#"{"level":"info","msg":"hi","drop":1,"user":{"id":7}}"#,
    )
    .unwrap();
    assert_eq!(
        out,
        json!({"level": "info", "text": "hi", "user": {}, "env": "dev", "meta": {"group": "/app", "region": "eu-west-1", "id": 7}})
    );
}

#[test]
fn a_parser_writes_under_a_destination_and_reads_a_source() {
    let config = json!([
        {"parseJSON": {"destination": "body"}},
        {"parseJSON": {"source": "body.inner", "destination": "inner"}},
    ]);
    let out = run(&config, r#"{"inner":"{\"a\":1}"}"#).unwrap();
    assert_eq!(out["body"]["inner"], json!("{\"a\":1}"));
    assert_eq!(out["inner"], json!({"a": 1}));
}

#[test]
fn csv_and_key_values_are_taken_apart() {
    let out = run(
        &json!([{"csv": {"columns": ["a", "b"]}}]),
        r#"x,"y, ""z""",w"#,
    )
    .unwrap();
    assert_eq!(out, json!({"a": "x", "b": "y, \"z\"", "column_3": "w"}));
    let out = run(
        &json!([{"csv": {"delimiter": ";", "quoteCharacter": "'"}}]),
        "1;'2;3'",
    )
    .unwrap();
    assert_eq!(out, json!({"column_1": "1", "column_2": "2;3"}));
    let config = json!([{"parseKeyValue": {"fieldDelimiter": " ", "keyValueDelimiter": ":", "keyPrefix": "k_", "nonMatchValue": "none"}}]);
    let out = run(&config, "a:1 b:two lone").unwrap();
    assert_eq!(out, json!({"k_a": "1", "k_b": "two", "k_lone": "none"}));
    assert_eq!(
        run(&json!([{"parseKeyValue": {}}]), "a=1&b=2").unwrap(),
        json!({"a": "1", "b": "2"})
    );
}

#[test]
fn grok_extracts_named_fields_as_text() {
    // No `:int` in the pattern (refused, as AWS refuses it): a typeConverter after it makes numbers.
    let config = json!([{"grok": {"match": "%{IPV4:ip} %{WORD:verb} %{URIPATHPARAM:path} %{NUMBER:status} %{NUMBER:secs}"}},
        {"typeConverter": {"entries": [{"key": "status", "type": "integer"}, {"key": "secs", "type": "double"}]}}]);
    let out = run(&config, "10.0.0.1 GET /a/b?c=1 200 0.25").unwrap();
    assert_eq!(
        out,
        json!({"ip": "10.0.0.1", "verb": "GET", "path": "/a/b?c=1", "status": 200, "secs": 0.25})
    );
    let apache =
        r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif HTTP/1.0" 200 2326"#;
    let out = run(&json!([{"grok": {"match": "%{COMMONAPACHELOG}"}}]), apache).unwrap();
    assert_eq!(out["clientip"], "127.0.0.1");
    assert_eq!(out["timestamp"], "10/Oct/2000:13:55:36 -0700");
    assert_eq!(out["request"], "/apache_pb.gif");
    assert_eq!(out["response"], "200");
    let out = run(&json!([{"grok": {"match": "%{TIMESTAMP_ISO8601:t} %{LOGLEVEL:level} %{GREEDYDATA:rest}"}}]), "2024-01-02T03:04:05Z ERROR boom here").unwrap();
    assert_eq!(
        out,
        json!({"t": "2024-01-02T03:04:05Z", "level": "ERROR", "rest": "boom here"})
    );
    assert_eq!(
        run(&json!([{"grok": {"match": "%{INT:n}"}}]), "no digits"),
        None
    );
}

#[test]
fn strings_are_cased_trimmed_split_substituted_and_converted() {
    let config = json!([
        {"parseJSON": {}},
        {"lowerCaseString": {"withKeys": ["a"]}},
        {"upperCaseString": {"withKeys": ["b"]}},
        {"trimString": {"withKeys": ["c"]}},
        {"splitString": {"entries": [{"source": "d", "delimiter": ","}]}},
        {"substituteString": {"entries": [{"source": "e", "from": "[0-9]+", "to": "#"}]}},
        {"typeConverter": {"entries": [{"key": "n", "type": "integer"}, {"key": "f", "type": "double"}, {"key": "t", "type": "boolean"}, {"key": "s", "type": "string"}, {"key": "bad", "type": "integer"}]}},
    ]);
    let out = run(&config, r#"{"a":"AbC","b":"xY","c":"  z  ","d":"1,2","e":"id 42 and 7","n":"12","f":"1.5","t":"TRUE","s":3,"bad":"x"}"#).unwrap();
    assert_eq!(
        out,
        json!({"a": "abc", "b": "XY", "c": "z", "d": ["1", "2"], "e": "id # and #", "n": 12, "f": 1.5, "t": true, "s": "3", "bad": "x"})
    );
}

#[test]
fn date_times_are_read_in_one_zone_and_written_in_another() {
    let config = json!([
        {"parseJSON": {}},
        {"dateTimeConverter": {"source": "when", "target": "iso", "matchPatterns": ["dd/MMM/yyyy:HH:mm:ss Z", "yyyy-MM-dd HH:mm:ss"]}},
        {"dateTimeConverter": {"source": "local", "target": "utc", "matchPatterns": ["yyyy-MM-dd hh:mm a"], "sourceTimezone": "America/New_York", "targetFormat": "yyyy-MM-dd HH:mm XXX"}},
        {"dateTimeConverter": {"source": "local", "target": "tokyo", "matchPatterns": ["yyyy-MM-dd hh:mm a"], "sourceTimezone": "America/New_York", "targetTimezone": "Asia/Tokyo", "targetFormat": "EEE, d MMMM yy HH:mm:ss.SSSSSS Z"}},
    ]);
    let out = run(
        &config,
        r#"{"when":"10/Oct/2000:13:55:36 -0700","local":"2024-07-04 01:30 PM"}"#,
    )
    .unwrap();
    assert_eq!(out["iso"], "2000-10-10T20:55:36.000Z");
    assert_eq!(out["utc"], "2024-07-04 17:30 Z");
    assert_eq!(out["tokyo"], "Fri, 5 July 24 02:30:00.000000 +0900");
}

#[test]
fn list_to_map_gathers_or_flattens() {
    let message =
        r#"{"list":[{"key":"a","value":"1"},{"key":"b","value":"2"},{"key":"b","value":"3"}]}"#;
    let gather = json!([{"parseJSON": {}}, {"listToMap": {"source": "list", "key": "key", "valueKey": "value", "target": "m"}}]);
    assert_eq!(
        run(&gather, message).unwrap()["m"],
        json!({"a": ["1"], "b": ["2", "3"]})
    );
    let first = json!([{"parseJSON": {}}, {"listToMap": {"source": "list", "key": "key", "valueKey": "value", "target": "m", "flatten": true, "flattenedElement": "first"}}]);
    assert_eq!(
        run(&first, message).unwrap()["m"],
        json!({"a": "1", "b": "2"})
    );
    let last = json!([{"parseJSON": {}}, {"listToMap": {"source": "list", "key": "key", "valueKey": "value", "target": "m", "flatten": true, "flattenedElement": "last"}}]);
    assert_eq!(
        run(&last, message).unwrap()["m"],
        json!({"a": "1", "b": "3"})
    );
}

#[test]
fn vpc_flow_logs_and_waf_logs_are_parsed() {
    let line = "2 123456789012 eni-0a1b 10.0.0.1 10.0.0.2 443 49152 6 10 840 1700000000 1700000060 ACCEPT OK";
    let out = run(&json!([{"parseVPC": {}}]), line).unwrap();
    assert_eq!(out["account_id"], "123456789012");
    assert_eq!(out["dstport"], 49152);
    assert_eq!(out["action"], "ACCEPT");
    assert_eq!(run(&json!([{"parseVPC": {}}]), "not a flow log"), None);
    assert_eq!(
        run(
            &json!([{"parseWAF": {"source": "@message"}}]),
            r#"{"action":"BLOCK"}"#
        )
        .unwrap(),
        json!({"action": "BLOCK"})
    );
}

#[test]
fn a_parser_that_fails_on_text_leaves_nothing() {
    assert_eq!(run(&json!([{"parseJSON": {}}]), "plain text"), None);
    // A later processor still runs over what is there.
    let out = run(
        &json!([{"parseJSON": {}}, {"addKeys": {"entries": [{"key": "k", "value": "v"}]}}]),
        "plain",
    )
    .unwrap();
    assert_eq!(out, json!({"k": "v"}));
}

#[test]
fn bad_configurations_are_refused() {
    assert!(refused(&json!({})).contains("list"));
    assert!(refused(&json!([])).contains("between 1 and 20"));
    assert!(refused(&json!(vec![json!({"parseJSON": {}}); 21])).contains("between 1 and 20"));
    assert!(refused(&json!([{}])).contains("exactly one"));
    assert!(refused(&json!([{"parseJSON": {}, "csv": {}}])).contains("exactly one"));
    assert!(later(json!({"nope": {}})).contains("Unknown processor"));
    // Recorded 2026-11-19.
    assert_eq!(
        refused(&json!([{"addKeys": {"entries": [{"key": "a", "value": "b"}]}}, {"parseVPC": {}}])),
        "Transformer config should begin with parser. Allowed parsers are parsePostgres, parseWAF, parseCloudfront, parseCloudfrontOCSF, parseRoute53, parseLambda, parseVPC, parseSyslog, parseRDS, parseCiscoFTD, parseF5BigIP, parseCiscoISE, parseForescoutNac, parseInfobloxDDI, parseToOCSF, parseJSON, grok, csv, parseKeyValue"
    );
    assert!(refused(&json!([{"parseJSON": {}}, {"parseVPC": {}}])).contains("first processor"));
    assert!(refused(&json!([{"parseVPC": {"source": "x"}}])).contains("only be @message"));
    for name in [
        "parseCloudfront",
        "parseRoute53",
        "parsePostgres",
        "parseToOCSF",
    ] {
        assert!(
            refused(&json!([{name: {}}])).contains("not run by doze-aws"),
            "{name}"
        );
    }
}

#[test]
fn bad_processor_settings_are_refused() {
    assert!(refused(&json!([{"grok": {"match": "%{NOPE:x}"}}])).contains("not known"));
    assert_eq!(
        refused(&json!([{"grok": {"match": "%{INT:x:int}"}}])),
        "Grok parser doesn't support type conversions for any grok patterns"
    );
    assert!(refused(&json!([{"grok": {"match": "(unclosed"}}])).contains("not a valid"));
    assert!(later(json!({"dateTimeConverter": {"source": "a", "target": "b", "matchPatterns": ["yyyy-QQ"]}})).contains("not valid"));
    assert!(later(json!({"dateTimeConverter": {"source": "a", "target": "b", "matchPatterns": ["yyyy"], "sourceTimezone": "Mars/Base"}})).contains("time zone"));
    assert!(
        later(json!({"typeConverter": {"entries": [{"key": "a", "type": "long"}]}}))
            .contains("integer, double")
    );
    assert!(
        later(json!({"listToMap": {"source": "a", "key": "k", "flatten": true}}))
            .contains("first or last")
    );
    assert!(later(json!({"addKeys": {"entries": []}})).contains("required"));
    assert!(
        later(json!({"substituteString": {"entries": [{"source": "a", "from": "[", "to": ""}]}}))
            .contains("not valid")
    );
    assert!(super::check(r#"[{"parseJSON":{}}]"#, r#"{"a":1}"#));
    assert!(!super::check("not json", "x"));
}

/// Recorded 2026-11-19: moveKeys puts the source into a target the event already has, and refuses one it does not.
#[test]
fn move_keys_moves_into_an_existing_key() {
    let meta = META;
    let t = |target: &str, overwrite: bool| {
        Transformer::parse(&json!([{"parseJSON": {}},
            {"moveKeys": {"entries": [{"source": "c", "target": target, "overwriteIfExists": overwrite}]}}]))
        .unwrap()
        .test(r#"{"a":1,"c":2}"#, &meta)
    };
    assert_eq!(
        t("d", false).unwrap_err(),
        "No corresponding field. Field Name: 'd', Processor: 'MoveKeysProcessor'"
    );
    let moved: Value = serde_json::from_str(&t("a", true).unwrap().unwrap()).unwrap();
    assert_eq!(moved, json!({"a": {"c": 2}}));
}
