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

fn granted(
    addr: &str,
    backend: &str,
    hosts: &[&str],
) -> doze_module::proto::msg::instance::Granted {
    doze_module::proto::msg::instance::Granted {
        addr: addr.into(),
        url: String::new(),
        hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
        backend: doze_module::proto::msg::instance::Address::Tcp(backend.into()),
    }
}

#[test]
fn the_server_under_doze_listens_and_serves_its_console_only_where_doze_granted() {
    let mut inst = instance();
    inst.listeners.insert(
        doze_module::proto::network::MAIN.into(),
        granted("local.shop.doze:4566", "127.0.0.1:41000", &[]),
    );
    inst.listeners.insert(
        super::CONSOLE.into(),
        granted(
            "local.shop.doze:4567",
            "127.0.0.1:41001",
            &["local.shop.doze"],
        ),
    );
    let args = super::server_args(&inst, "us-east-1").unwrap();
    assert_eq!(args[..2], ["--listen", "127.0.0.1:41000"]);
    let at = args.iter().position(|a| a == "--console").unwrap();
    assert_eq!(args[at + 1], "127.0.0.1:41001", "{args:?}");
    let at = args
        .iter()
        .position(|a| a == "--console-allow-host")
        .unwrap();
    assert_eq!(args[at + 1], "local.shop.doze", "{args:?}");
    assert!(!args.iter().any(|a| a == "--no-console"), "{args:?}");
    let at = args.iter().position(|a| a == "--advertise").unwrap();
    assert_eq!(args[at + 1], "http://local.shop.doze:4566", "{args:?}");
    // The console says it is where doze put it (the front), not at the private backend it listens on.
    let at = args
        .iter()
        .position(|a| a == "--advertise-console")
        .unwrap();
    assert_eq!(args[at + 1], "http://local.shop.doze:4567", "{args:?}");
}

#[test]
fn clients_are_told_the_address_doze_gave_never_the_backend_it_picked() {
    // Names off: the stack's address on loopback; the backend is another port that doze picks again at every start.
    let mut inst = instance();
    inst.listeners.insert(
        doze_module::proto::network::MAIN.into(),
        granted("127.0.0.1:24566", "127.0.0.1:27538", &[]),
    );
    let args = super::server_args(&inst, "us-east-1").unwrap();
    let at = args.iter().position(|a| a == "--advertise").unwrap();
    assert_eq!(args[at + 1], "http://127.0.0.1:24566", "{args:?}");
    assert!(
        !args
            .iter()
            .any(|a| a.contains("27538") && a != "127.0.0.1:27538"),
        "{args:?}"
    );
}

#[test]
fn without_a_console_listener_granted_the_server_runs_without_its_console() {
    let mut inst = instance();
    inst.listeners.insert(
        doze_module::proto::network::MAIN.into(),
        granted("local.shop.doze:4566", "127.0.0.1:41000", &[]),
    );
    let args = super::server_args(&inst, "us-east-1").unwrap();
    assert_eq!(args[..2], ["--listen", "127.0.0.1:41000"]);
    assert!(args.iter().any(|a| a == "--no-console"), "{args:?}");
    assert!(!args.iter().any(|a| a == "--console"), "{args:?}");
}

#[test]
fn the_console_is_its_own_listener_on_the_instances_name_and_the_app_can_open_it() {
    use doze_module::Module;
    let listeners = super::listeners();
    let names: Vec<&str> = listeners.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["main", "console"]);
    let console = &listeners[1];
    assert!(
        console.http && console.tcp && console.port != 0,
        "{console:?}"
    );
    assert_eq!(console.routes, ["@"]);
    assert!(
        doze_module::proto::network::validate(&listeners).is_empty(),
        "{:?}",
        doze_module::proto::network::validate(&listeners)
    );
    let spec = super::Aws.about().console.unwrap();
    assert_eq!(spec.web_url.as_deref(), Some("http://{address:console}"));
    assert_eq!(spec.front.as_deref(), Some("web"));
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

#[test]
fn a_bucket_name_s3_would_refuse_is_refused_at_its_block_before_boot() {
    use super::spec::Bucket;
    let bucket = |name: &str| Bucket {
        name: name.into(),
        ..Bucket::default()
    };
    let s = AwsSpec {
        bucket: vec![
            bucket("uploads"),
            bucket("x"),
            bucket("My_Bucket"),
            bucket("-lead"),
            bucket(&"a".repeat(64)),
            bucket("my-bucket.logs"),
        ],
        ..AwsSpec::default()
    };
    let d = check::check(&s, "");
    // The four bad names, each at its own block, and the message names the rule and the name.
    let at: Vec<_> = d.iter().map(|d| format!("{:?}", d.path)).collect();
    assert_eq!(d.len(), 4, "{at:#?}");
    for (diag, name) in d.iter().zip(["x", "My_Bucket", "-lead"]) {
        assert!(
            diag.message.contains(&format!("{name:?}"))
                && diag.message.contains("3 to 63 characters")
                && diag
                    .message
                    .contains("lowercase letters, digits, dots and hyphens")
                && diag
                    .message
                    .contains("starting and ending with a letter or digit"),
            "{}",
            diag.message
        );
    }
    let paths: Vec<_> = d.iter().map(|d| d.path.clone()).collect();
    assert_eq!(
        paths,
        [1, 2, 3, 4].map(|i| vec![check::key("bucket"), check::idx(i)])
    );
}

fn decoded(hcl: &str) -> AwsSpec {
    let h = doze_module_test::hcl::parse_block(hcl, &schema()).unwrap();
    decode(&doze_module::proto::msg::instance::Instance {
        spec: h.spec,
        ..instance()
    })
}

#[test]
fn the_region_is_us_east_1_unless_the_block_says_and_it_is_what_the_server_is_given() {
    let none = decoded("aws \"local\" {}");
    assert_eq!(none.region(), "us-east-1");
    assert_eq!(
        AwsSpec::default().region(),
        "us-east-1",
        "a block built with none"
    );
    let london = decoded("aws \"local\" {\n  region = \"eu-west-2\"\n}");
    assert_eq!(london.region(), "eu-west-2");
    let args = |region: &str| super::server_args(&instance_at_backend(), region).unwrap();
    let at = |args: &[String]| {
        args.iter()
            .position(|a| a == "--region")
            .map(|i| args[i + 1].clone())
    };
    assert_eq!(at(&args(none.region())).as_deref(), Some("us-east-1"));
    assert_eq!(at(&args(london.region())).as_deref(), Some("eu-west-2"));
}

fn instance_at_backend() -> doze_module::proto::msg::instance::Instance {
    use doze_module::proto::msg::instance::{Address, Granted};
    let mut i = instance();
    i.listeners.insert(
        doze_module::proto::network::MAIN.into(),
        Granted {
            addr: "127.0.0.1:46400".into(),
            url: String::new(),
            hosts: vec![],
            backend: Address::Tcp("127.0.0.1:46401".into()),
        },
    );
    i
}

#[test]
fn a_region_aws_does_not_have_is_one_diagnostic_at_the_attribute_listing_the_valid_ones() {
    let bad = decoded("aws \"local\" {\n  region = \"us-east-9\"\n}");
    let d = check::check(&bad, "");
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].path, vec![check::key("region")]);
    assert!(
        d[0].message.contains("\"us-east-9\" is not an AWS Region"),
        "{}",
        d[0].message
    );
    let hint = d[0].hint.as_deref().unwrap();
    assert!(
        hint.starts_with("one of: ") && hint.contains("eu-west-2") && hint.contains("us-west-2"),
        "{hint}"
    );
    for ok in ["us-east-1", "eu-west-2", "ap-southeast-2", "us-gov-west-1"] {
        let s = decoded(&format!("aws \"local\" {{\n  region = \"{ok}\"\n}}"));
        assert!(check::check(&s, "").is_empty(), "{ok}");
    }
}

#[test]
fn the_region_is_in_the_schema_with_its_default_and_the_client_takes_it_as_a_placeholder() {
    use doze_module::Module;
    let attr = schema()
        .attrs
        .into_iter()
        .find(|a| a.name == "region")
        .unwrap();
    assert!(attr.doc.contains("module.<name>.region"), "{}", attr.doc);
    assert_eq!(format!("{:?}", attr.default), "Some(String(\"us-east-1\"))");
    let c = super::Aws.about().client.unwrap();
    assert_eq!(c.env["AWS_DEFAULT_REGION"], "{region}");
}
