#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::ffi::OsString;

use serde_json::json;

use super::spec::{AwsSpec, Function, Queue, Rule, Subscription, Table, Topic};
use super::template::{id, template};
use super::{check, is_module};

fn os(v: &[&str]) -> Vec<OsString> {
    v.iter().map(OsString::from).collect()
}

#[test]
fn it_is_the_module_as_aws_plugin_or_doze_aws_module_and_the_server_otherwise() {
    assert!(is_module(&os(&["/x/bin/aws-plugin"])));
    assert!(is_module(&os(&["aws-plugin", "--describe"])));
    assert!(is_module(&os(&["doze-aws", "module"])));
    assert!(!is_module(&os(&["aws-plugin", "--listen", "127.0.0.1:1"])));
    assert!(!is_module(&os(&["doze-aws"])));
}

#[test]
fn logical_ids_are_stable_and_never_meet() {
    assert!(id("Queue", "emails").starts_with("Queueemails"));
    // A derived id ("…Dlq") can never be another block's: every block id ends in eight hex digits.
    assert_ne!(format!("{}Dlq", id("Queue", "a")), id("Queue", "aDlq"));
    assert_ne!(id("Queue", "a-b"), id("Queue", "ab"));
    assert_eq!(id("Queue", "a-b"), id("Queue", "a-b"));
    assert!(
        id("Parameter", "/app/x")
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
    );
}

fn queue(name: &str) -> Queue {
    Queue {
        name: name.into(),
        ..Queue::default()
    }
}

#[test]
fn a_queue_with_an_auto_dlq_is_two_queues_wired_and_fifo_names_end_in_fifo() {
    let s = AwsSpec {
        queue: vec![
            Queue {
                dlq: Some("auto".into()),
                fifo: true,
                ..queue("orders")
            },
            Queue {
                dlq: Some("orders".into()),
                max_receives: Some(5),
                ..queue("jobs")
            },
        ],
        ..AwsSpec::default()
    };
    let t = template(&s, "");
    let r = &t["Resources"];
    let main = &r[id("Queue", "orders")]["Properties"];
    assert_eq!(main["QueueName"], "orders.fifo");
    assert_eq!(main["RedrivePolicy"]["maxReceiveCount"], 3);
    let dlq = &r[format!("{}Dlq", id("Queue", "orders"))];
    assert_eq!(dlq["Properties"]["QueueName"], "orders-dlq.fifo");
    assert_eq!(dlq["DeletionPolicy"], "Retain");
    let jobs = &r[id("Queue", "jobs")]["Properties"]["RedrivePolicy"];
    assert_eq!(
        jobs["deadLetterTargetArn"],
        json!({ "Fn::GetAtt": [id("Queue", "orders"), "Arn"] })
    );
    assert_eq!(jobs["maxReceiveCount"], 5);
}

#[test]
fn a_table_key_becomes_its_attributes_and_schema() {
    let s = AwsSpec {
        table: vec![Table {
            name: "sessions".into(),
            key: "pk:S sk:N".into(),
            ttl: Some("exp".into()),
            ..Table::default()
        }],
        ..AwsSpec::default()
    };
    let p = &template(&s, "")["Resources"][id("Table", "sessions")]["Properties"];
    assert_eq!(
        p["KeySchema"][1],
        json!({ "AttributeName": "sk", "KeyType": "RANGE" })
    );
    assert_eq!(p["AttributeDefinitions"][1]["AttributeType"], "N");
    assert_eq!(p["TimeToLiveSpecification"]["AttributeName"], "exp");
}

#[test]
fn the_example_passes_its_checks_and_every_resource_is_kept_on_removal() {
    let h = doze_module_test::hcl::parse_block(super::EXAMPLE, &schema()).unwrap();
    let inst = doze_module::proto::msg::instance::Instance {
        spec: h.spec,
        ..instance()
    };
    let spec = decode(&inst);
    assert!(check::check(&spec, "").is_empty());
    let t = template(&spec, "");
    for (id, r) in t["Resources"].as_object().unwrap() {
        if id != "DozeKept" {
            assert_eq!(r["DeletionPolicy"], "Retain", "{id}");
        }
    }
}

fn schema() -> doze_module::proto::msg::hello::Schema {
    <AwsSpec as doze_module::spec::Spec>::schema()
}

fn instance() -> doze_module::proto::msg::instance::Instance {
    doze_module::proto::msg::instance::Instance {
        name: "local".into(),
        engine: "aws".into(),
        version: String::new(),
        data_dir: "/d".into(),
        socket_dir: "/s".into(),
        spec: doze_module::proto::msg::value::Node {
            span: doze_module::proto::msg::value::Span::default(),
            value: doze_module::proto::msg::value::Value::Map(BTreeMap::new()),
        },
        host: "local.shop.doze".into(),
        listeners: BTreeMap::new(),
        deps: vec![],
        toolchain: None,
        dir: String::new(),
    }
}

fn decode(inst: &doze_module::proto::msg::instance::Instance) -> AwsSpec {
    let mut cx = doze_module::spec::Decoding::new("1");
    let s = <AwsSpec as doze_module::spec::Spec>::decode(&inst.spec, &mut cx).unwrap();
    assert!(cx.diagnostics.is_empty(), "{:?}", cx.diagnostics);
    s
}

#[test]
fn references_keys_json_and_code_are_checked_at_their_line() {
    let s = AwsSpec {
        queue: vec![Queue {
            dlq: Some("nope".into()),
            content_dedup: true,
            ..queue("a")
        }],
        topic: vec![Topic {
            name: "t".into(),
            subscribe: vec![Subscription {
                queue: Some("a".into()),
                lambda: Some("f".into()),
                filter: Some("{not json".into()),
                ..Subscription::default()
            }],
            ..Topic::default()
        }],
        table: vec![Table {
            name: "x".into(),
            key: "pk:Q".into(),
            ..Table::default()
        }],
        function: vec![Function {
            name: "f".into(),
            code: "/no/such/dir".into(),
            ..Function::default()
        }],
        rule: vec![Rule {
            name: "r".into(),
            targets: vec!["queue:missing".into(), "sqs:a".into()],
            ..Rule::default()
        }],
        ..AwsSpec::default()
    };
    let d = check::check(&s, "");
    let msgs: Vec<&str> = d.iter().map(|d| d.message.as_str()).collect();
    for want in [
        "no queue \"nope\" is declared",
        "content-based deduplication is for FIFO queues",
        "set exactly one of queue, lambda, http",
        "not JSON",
        "\"pk:Q\" is not a key",
        "/no/such/dir is not a directory",
        "needs a pattern or a schedule",
        "no queue \"missing\" is declared in this block",
        "\"sqs:a\" is not a target",
    ] {
        assert!(msgs.iter().any(|m| m.contains(want)), "{want}: {msgs:#?}");
    }
    assert_eq!(d.len(), 9, "{msgs:#?}");
}

#[test]
fn the_shell_client_is_the_aws_cli_at_the_instance() {
    use doze_module::Module;
    let about = super::Aws.about();
    let c = about.client.unwrap();
    assert_eq!(c.program, "aws");
    assert_eq!(c.env["AWS_ENDPOINT_URL"], "http://{host}:{port}");
    assert!(about.detect.unwrap().packages["pypi"].contains(&"boto3".to_string()));
}
