//! The `region` of the aws block, on the module as doze drives it: what the block declares is made in that Region, a client of another
//! Region is told where it is, the Region reaches the variables and the references, and a Region AWS does not have is refused at its
//! line. Needs the `aws` CLI as module_boot does.
#![cfg(feature = "module")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod module_support;

use std::time::Duration;

use doze_module_proto::msg::value::Value;
use module_support::{aws_fails, aws_in, harness, have_cli};

const LONDON: &str = "aws \"local\" {\n  region = \"eu-west-2\"\n  queue \"pick-lists\" {}\n  bucket \"fulfilment\" {}\n}\n";

#[tokio::test(flavor = "current_thread")]
async fn a_block_in_eu_west_2_makes_its_queue_there_and_a_us_east_1_client_is_told_where_it_is() {
    let b = harness().boot(LONDON).await.unwrap();
    if have_cli() {
        let url = |region: &str| {
            aws_in(
                &b,
                region,
                &["sqs", "get-queue-url", "--queue-name", "pick-lists"],
            )["QueueUrl"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert!(url("eu-west-2").ends_with("/pick-lists"));
        let said = aws_fails(
            &b,
            "us-east-1",
            &["sqs", "get-queue-url", "--queue-name", "pick-lists"],
        );
        assert!(
            said.contains("NonExistentQueue"),
            "the code is unchanged: {said}"
        );
        assert!(
            said.contains("exists in eu-west-2; this client asked for us-east-1")
                && said.contains("set `region` on the aws block or the client's region"),
            "{said}"
        );
        // A queue nobody has gets the plain answer.
        let plain = aws_fails(
            &b,
            "us-east-1",
            &["sqs", "get-queue-url", "--queue-name", "nobody"],
        );
        assert!(!plain.contains("exists in"), "{plain}");
        // The bucket is kept in the same Region, and is told the same way.
        aws_in(
            &b,
            "eu-west-2",
            &["s3api", "head-bucket", "--bucket", "fulfilment"],
        );
        let said = aws_fails(
            &b,
            "us-east-1",
            &["s3api", "list-objects-v2", "--bucket", "fulfilment"],
        );
        assert!(said.contains("NoSuchBucket"), "{said}");
        assert!(
            said.contains("exists in eu-west-2; this client asked for us-east-1"),
            "{said}"
        );
    }
    b.stop(Duration::from_secs(20)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_region_reaches_the_clients_variables_and_the_reference() {
    let b = harness().boot(LONDON).await.unwrap();
    let conn = b.module.connection(&b.instance).await.unwrap();
    assert_eq!(conn.env["AWS_REGION"], "eu-west-2");
    assert_eq!(conn.env["AWS_DEFAULT_REGION"], "eu-west-2");
    let attrs = b.module.attributes(&b.instance).await.unwrap();
    assert_eq!(
        attrs["region"],
        Value::String("eu-west-2".into()),
        "module.local.region"
    );
    b.stop(Duration::from_secs(20)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_block_with_no_region_is_in_us_east_1_and_adds_no_variable() {
    let b = harness()
        .boot("aws \"local\" {\n  queue \"jobs\" {}\n}\n")
        .await
        .unwrap();
    let conn = b.module.connection(&b.instance).await.unwrap();
    assert!(conn.env.is_empty(), "{:?}", conn.env);
    let attrs = b.module.attributes(&b.instance).await.unwrap();
    assert_eq!(attrs["region"], Value::String("us-east-1".into()));
    if have_cli() {
        let url = aws_in(
            &b,
            "us-east-1",
            &["sqs", "get-queue-url", "--queue-name", "jobs"],
        );
        assert!(url["QueueUrl"].as_str().unwrap().ends_with("/jobs"));
    }
    b.stop(Duration::from_secs(20)).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_region_aws_does_not_have_is_refused_at_its_line_with_the_valid_ones() {
    let e = harness()
        .boot("aws \"local\" {\n  region = \"eu-west-9\"\n}\n")
        .await
        .unwrap_err();
    assert!(e.contains("\"eu-west-9\" is not an AWS Region"), "{e}");
    assert!(
        e.contains("one of: ") && e.contains("eu-west-2") && e.contains("us-east-1"),
        "{e}"
    );
    // The path is the attribute's, which the core turns into its line in doze.hcl.
    assert!(e.contains("at [Key(\"region\")]"), "at the attribute: {e}");
}
