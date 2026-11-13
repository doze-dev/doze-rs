//! Code run in place (`S3Bucket: _local_`): a change to the directory reaches `$LATEST`, and a published version keeps what it froze.

use aws_core::base64;
use serde_json::json;

use super::{ACCOUNT, World, have};

#[tokio::test]
async fn code_run_in_place_is_picked_up_when_the_directory_changes() {
    if !have("python3") {
        return;
    }
    let w = World::new();
    let dir = std::env::temp_dir().join(format!("doze-lambda-local-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.py"), "def handler(e, c):\n    return 1\n").unwrap();
    w.ok(
        "CreateFunction",
        json!({"FunctionName": "l", "Runtime": "python3.12", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"), "Handler": "index.handler",
            "Code": {"S3Bucket": "_local_", "S3Key": dir.to_string_lossy()}}),
    )
    .await;
    assert_eq!(w.invoke("l", &json!({})).await["Payload"], 1);
    let v1 = w.ok("PublishVersion", json!({"FunctionName": "l"})).await["Version"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    std::fs::write(
        dir.join("index.py"),
        "def handler(e, c):\n    return 2\n  \n",
    )
    .unwrap();
    assert_eq!(w.invoke("l", &json!({})).await["Payload"], 2);
    // The published version is a copy: the directory changed under `$LATEST` and not under it.
    let frozen = w
        .ok(
            "Invoke",
            json!({"FunctionName": "l", "Qualifier": v1, "Payload": base64::encode(b"{}")}),
        )
        .await;
    assert_eq!(
        base64::decode(frozen["Payload"].as_str().unwrap()).unwrap(),
        b"1"
    );
    drop(std::fs::remove_dir_all(&dir));
    w.stop().await;
}
