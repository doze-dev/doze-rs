//! Exports written to S3 in AWS's layout, and imports read back.

use std::io::Read as _;

use serde_json::{Value, json};

use super::bus::Bus;

fn gunzip(bytes: &[u8]) -> String {
    let mut out = String::new();
    flate2::read::MultiGzDecoder::new(bytes)
        .read_to_string(&mut out)
        .unwrap();
    out
}

async fn with_recovery(bus: &Bus, name: &str) {
    bus.table(name).await;
    bus.ok(
        "UpdateContinuousBackups",
        json!({"TableName": name, "PointInTimeRecoverySpecification": {"PointInTimeRecoveryEnabled": true}}),
    )
    .await;
}

#[tokio::test]
async fn an_export_is_written_in_the_layout_aws_writes_and_imports_back() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    for (pk, n) in [("a", "1"), ("b", "2")] {
        bus.ok("PutItem", json!({"TableName": "tab", "Item": {"pk": {"S": pk}, "n": {"N": n}, "s": {"SS": ["x"]}}}))
            .await;
    }
    bus.s3("CreateBucket", json!({"Bucket": "exports"})).await;
    let arn = "arn:aws:dynamodb:us-east-1:000000000000:table/tab";
    let out = bus
        .ok(
            "ExportTableToPointInTime",
            json!({"TableArn": arn, "S3Bucket": "exports", "S3Prefix": "nightly/"}),
        )
        .await;
    assert_eq!(out["ExportDescription"]["ExportStatus"], "IN_PROGRESS");
    let export_arn = out["ExportDescription"]["ExportArn"]
        .as_str()
        .unwrap()
        .to_owned();
    let id = export_arn.rsplit('/').next().unwrap().to_owned();
    let done = bus
        .ok("DescribeExport", json!({"ExportArn": export_arn}))
        .await;
    let d = &done["ExportDescription"];
    assert_eq!(d["ExportStatus"], "COMPLETED", "{d}");
    assert_eq!(d["ItemCount"], 2);
    assert_eq!(d["ExportFormat"], "DYNAMODB_JSON");
    let dir = format!("nightly/AWSDynamoDB/{id}");
    assert_eq!(d["ExportManifest"], format!("{dir}/manifest-summary.json"));
    check_layout(&bus, &dir, &export_arn).await;
    let listed = bus.ok("ListExports", json!({"TableArn": arn})).await;
    assert_eq!(listed["ExportSummaries"][0]["ExportStatus"], "COMPLETED");
    import_back(&bus, &dir).await;
}

/// The files of a finished export, and what the manifests and the data say.
async fn check_layout(bus: &Bus, dir: &str, export_arn: &str) {
    let mut keys = bus.keys("exports").await;
    keys.sort();
    let data_key = keys.iter().find(|k| k.contains("/data/")).unwrap().clone();
    assert!(data_key.starts_with(&format!("{dir}/data/")) && data_key.ends_with(".json.gz"));
    for name in [
        "_started",
        "manifest-files.json",
        "manifest-files.md5",
        "manifest-summary.json",
        "manifest-summary.md5",
    ] {
        assert!(
            keys.contains(&format!("{dir}/{name}")),
            "{name} in {keys:?}"
        );
    }
    let summary: Value = serde_json::from_slice(
        &bus.object("exports", &format!("{dir}/manifest-summary.json"))
            .await,
    )
    .unwrap();
    assert_eq!(summary["exportArn"], export_arn);
    assert_eq!(summary["itemCount"], 2);
    assert_eq!(summary["outputFormat"], "DYNAMODB_JSON");
    assert_eq!(
        summary["manifestFilesS3Key"],
        format!("{dir}/manifest-files.json")
    );
    let files: Value = serde_json::from_slice(
        &bus.object("exports", &format!("{dir}/manifest-files.json"))
            .await,
    )
    .unwrap();
    assert_eq!(files["dataFileS3Key"], data_key.as_str());
    assert_eq!(files["itemCount"], 2);
    let lines = gunzip(&bus.object("exports", &data_key).await);
    let mut items: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    items.sort_by_key(|i| i["Item"]["pk"]["S"].as_str().unwrap().to_owned());
    assert_eq!(
        items[0],
        json!({"Item": {"pk": {"S": "a"}, "n": {"N": "1"}, "s": {"SS": ["x"]}}})
    );
}

/// The export's data folder imports into a new table.
async fn import_back(bus: &Bus, dir: &str) {
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "exports", "S3KeyPrefix": format!("{dir}/data/")},
                "InputFormat": "DYNAMODB_JSON", "InputCompressionType": "GZIP",
                "TableCreationParameters": {"TableName": "copy", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let import_arn = imported["ImportTableDescription"]["ImportArn"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        imported["ImportTableDescription"]["ImportStatus"],
        "IN_PROGRESS"
    );
    let d = bus
        .ok("DescribeImport", json!({"ImportArn": import_arn}))
        .await;
    assert_eq!(
        d["ImportTableDescription"]["ImportStatus"], "COMPLETED",
        "{d}"
    );
    assert_eq!(d["ImportTableDescription"]["ImportedItemCount"], 2);
    let got = bus
        .ok(
            "GetItem",
            json!({"TableName": "copy", "Key": {"pk": {"S": "b"}}}),
        )
        .await;
    assert_eq!(got["Item"]["n"], json!({"N": "2"}));
    let t = bus.ok("DescribeTable", json!({"TableName": "copy"})).await;
    assert_eq!(t["Table"]["TableStatus"], "ACTIVE");
    assert_eq!(
        bus.ok("ListImports", json!({})).await["ImportSummaryList"][0]["ImportStatus"],
        "COMPLETED"
    );
}

#[tokio::test]
async fn ion_exports_and_csv_imports() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    bus.ok(
        "PutItem",
        json!({"TableName": "tab", "Item": {"pk": {"S": "a"}, "n": {"N": "600"}}}),
    )
    .await;
    bus.s3("CreateBucket", json!({"Bucket": "bkt"})).await;
    let out = bus
        .ok("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/tab", "S3Bucket": "bkt", "ExportFormat": "ION"}))
        .await;
    let id = out["ExportDescription"]["ExportArn"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned();
    let keys = bus.keys("bkt").await;
    let data = keys.iter().find(|k| k.ends_with(".ion.gz")).unwrap();
    assert!(data.starts_with(&format!("AWSDynamoDB/{id}/data/")));
    assert_eq!(
        gunzip(&bus.object("bkt", data).await),
        "$ion_1_0 {Item:{n:600.,pk:\"a\"}}\n"
    );

    bus.put_object(
        "bkt",
        "csv/one.csv",
        b"pk,n,note\nx,1,hello\ny,2,\n,3,no key\n",
    )
    .await;
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "bkt", "S3KeyPrefix": "csv/"}, "InputFormat": "CSV",
                "TableCreationParameters": {"TableName": "fromcsv", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let arn = imported["ImportTableDescription"]["ImportArn"].clone();
    let d =
        bus.ok("DescribeImport", json!({"ImportArn": arn})).await["ImportTableDescription"].clone();
    assert_eq!(
        (d["ImportedItemCount"].as_u64(), d["ErrorCount"].as_u64()),
        (Some(2), Some(1)),
        "{d}"
    );
    let y = bus
        .ok(
            "GetItem",
            json!({"TableName": "fromcsv", "Key": {"pk": {"S": "y"}}}),
        )
        .await;
    assert_eq!(y["Item"], json!({"pk": {"S": "y"}, "n": {"S": "2"}}));
}

#[tokio::test]
async fn an_export_or_import_without_a_bucket_fails_and_says_why() {
    let bus = Bus::new();
    with_recovery(&bus, "tab").await;
    let out = bus
        .ok("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/tab", "S3Bucket": "nope"}))
        .await;
    let d = bus
        .ok(
            "DescribeExport",
            json!({"ExportArn": out["ExportDescription"]["ExportArn"]}),
        )
        .await;
    assert_eq!(d["ExportDescription"]["ExportStatus"], "FAILED");
    assert_eq!(d["ExportDescription"]["FailureCode"], "S3NoSuchBucket");
    let imported = bus
        .ok(
            "ImportTable",
            json!({"S3BucketSource": {"S3Bucket": "nope"}, "InputFormat": "DYNAMODB_JSON",
                "TableCreationParameters": {"TableName": "never", "BillingMode": "PAY_PER_REQUEST",
                    "KeySchema": [{"AttributeName": "pk", "KeyType": "HASH"}],
                    "AttributeDefinitions": [{"AttributeName": "pk", "AttributeType": "S"}]}}),
        )
        .await;
    let d = bus
        .ok(
            "DescribeImport",
            json!({"ImportArn": imported["ImportTableDescription"]["ImportArn"]}),
        )
        .await;
    assert_eq!(d["ImportTableDescription"]["ImportStatus"], "FAILED");
    let e = bus
        .call("DescribeTable", json!({"TableName": "never"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "ResourceNotFoundException");
    // Exports need recovery on.
    bus.table("plain").await;
    let e = bus
        .call("ExportTableToPointInTime", json!({"TableArn": "arn:aws:dynamodb:us-east-1:000000000000:table/plain", "S3Bucket": "bkt"}))
        .await
        .unwrap_err();
    assert_eq!(e.code, "PointInTimeRecoveryUnavailableException");
}
