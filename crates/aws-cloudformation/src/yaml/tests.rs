#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::{YamlError, parse};

fn at(line: usize, column: usize) -> YamlError {
    YamlError { line, column }
}

#[test]
fn a_template_with_short_forms() {
    let text = "AWSTemplateFormatVersion: '2010-09-09'\nDescription: >-\n  two\n  lines\nParameters:\n  Env: {Type: String, Default: dev}\nResources:\n  Q:\n    Type: AWS::SQS::Queue # a queue\n    Properties:\n      QueueName: !Sub '${AWS::StackName}-q'\n      Delay: 5\n      Tags:\n        - Key: k\n          Value: !Ref Env\n        - Key: arn\n          Value: !GetAtt Other.Arn\n      Cond: !If\n        - IsProd\n        - !Join ['', [a, !Ref Env]]\n        - !Ref AWS::NoValue\n";
    let v = parse(text).unwrap();
    assert_eq!(v["AWSTemplateFormatVersion"], "2010-09-09");
    assert_eq!(v["Description"], "two lines");
    assert_eq!(
        v["Parameters"]["Env"],
        json!({"Type": "String", "Default": "dev"})
    );
    let p = &v["Resources"]["Q"]["Properties"];
    assert_eq!(p["QueueName"], json!({"Fn::Sub": "${AWS::StackName}-q"}));
    assert_eq!(p["Delay"], 5);
    assert_eq!(p["Tags"][0], json!({"Key": "k", "Value": {"Ref": "Env"}}));
    assert_eq!(
        p["Tags"][1]["Value"],
        json!({"Fn::GetAtt": ["Other", "Arn"]})
    );
    assert_eq!(
        p["Cond"],
        json!({"Fn::If": ["IsProd", {"Fn::Join": ["", ["a", {"Ref": "Env"}]]}, {"Ref": "AWS::NoValue"}]})
    );
}

#[test]
fn scalars_resolve_as_yaml_one_one_does() {
    let v = parse("a: yes\nb: 012\nc: 1.5\nd: 1.10\ne: ~\nf: -3\ng: 2012-10-17\nh: 'it''s'\ni: \"tab\\there \\u00e9\"\nj: http://x:80/y\n").unwrap();
    assert_eq!(
        v,
        json!({"a": true, "b": "012", "c": 1.5, "d": "1.10", "e": null, "f": -3, "g": "2012-10-17", "h": "it's", "i": "tab\there é", "j": "http://x:80/y"})
    );
}

#[test]
fn block_scalars_keep_or_fold_their_lines() {
    let v = parse("a: |\n  one\n    two\n\n  three\nb: >\n  one\n  two\n\n  three\nc: |-\n  x\nd: |+\n  y\n\ne: z\n").unwrap();
    assert_eq!(v["a"], "one\n  two\n\nthree\n");
    assert_eq!(v["b"], "one two\nthree\n");
    assert_eq!(v["c"], "x");
    assert_eq!(v["d"], "y\n\n");
    assert_eq!(v["e"], "z");
}

#[test]
fn sequences_nest_and_compact() {
    let v = parse("- - a\n  - b\n- k: 1\n  j: 2\n-\n  - c\nkey: 1\n").err();
    assert!(v.is_some(), "a mapping cannot follow a top-level sequence");
    let v = parse("x:\n- a\n- b\ny:\n  - - c\n").unwrap();
    assert_eq!(v, json!({"x": ["a", "b"], "y": [["c"]]}));
    let v = parse("- - a\n  - b\n- k: 1\n  j: 2\n").unwrap();
    assert_eq!(v, json!([["a", "b"], {"k": 1, "j": 2}]));
}

#[test]
fn multi_line_plain_and_flow() {
    let v = parse("a: one\n  two\n\n  three\nb: [x, y,\n  z]\nc: {p: 1,\n  q: [2]}\n").unwrap();
    assert_eq!(
        v,
        json!({"a": "one two\nthree", "b": ["x", "y", "z"], "c": {"p": 1, "q": [2]}})
    );
}

#[test]
fn anchors_and_aliases_expand_within_a_budget() {
    let v = parse("a: &x {k: v}\nb: *x\n").unwrap();
    assert_eq!(v["b"], json!({"k": "v"}));
    let mut bomb = String::from("a: &a [x, x, x, x, x, x, x, x, x, x]\n");
    for (i, prev) in ["b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .zip(["a", "b", "c", "d", "e", "f", "g"])
    {
        bomb.push_str(&format!("{i}: &{i} [*{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}, *{prev}]\n"));
    }
    assert!(parse(&bomb).is_err());
    assert!(parse("a: *nope\n").is_err());
}

#[test]
fn errors_name_the_line_and_column_as_aws_does() {
    // Recorded: `{nope` is "YAML not well-formed. (line 1, column 6)".
    assert_eq!(parse("{nope").unwrap_err(), at(1, 6));
    assert!(parse("a: b: c\n").is_err());
    assert!(parse("a: 1\na: 2\n").is_err());
    assert!(parse("a:\n\t- x\n").is_err());
    assert!(parse("a: 'open\n").is_err());
    let deep = "[".repeat(500);
    assert!(parse(&deep).is_err());
}

#[test]
fn json_is_yaml() {
    let v = parse("{\"Resources\": {\"Q\": {\"Type\": \"AWS::SQS::Queue\", \"Properties\": {\"N\": [1, 2.5, true, null]}}}}").unwrap();
    assert_eq!(
        v["Resources"]["Q"]["Properties"]["N"],
        json!([1, 2.5, true, null])
    );
}

#[test]
fn documents_and_comments() {
    let v = parse("%YAML 1.1\n---\n# c\na: 1 # trailing\n...\n").unwrap();
    assert_eq!(v, json!({"a": 1}));
    assert!(parse("a: 1\n---\nb: 2\n").is_err());
    assert_eq!(parse("").unwrap(), serde_json::Value::Null);
    assert_eq!(parse("a: x#y\n").unwrap(), json!({"a": "x#y"}));
}

#[test]
fn tags_on_their_own_line_take_the_block_below() {
    let v = parse("a: !Sub\n  - '${x}'\n  - x: !Ref Y\nb: !GetAZs\nc: !Select [0, !GetAZs '']\n")
        .unwrap();
    assert_eq!(v["a"], json!({"Fn::Sub": ["${x}", {"x": {"Ref": "Y"}}]}));
    assert_eq!(v["b"], json!({"Fn::GetAZs": ""}));
    assert_eq!(v["c"], json!({"Fn::Select": [0, {"Fn::GetAZs": ""}]}));
}
