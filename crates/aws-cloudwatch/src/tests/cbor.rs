//! The CBOR protocol around this service's model: a request body in CBOR reads as the JSON the model checks, and an answer tree
//! written as CBOR reads back as the JSON it stands for.

use aws_model_cloudwatch::tables::TABLES;
use aws_schema::cbor::{map_header, text};
use serde_json::json;

use super::{NOW, World};

fn float(out: &mut Vec<u8>, f: f64) {
    out.push(0xfb);
    out.extend_from_slice(&f.to_be_bytes());
}

#[test]
fn a_cbor_put_metric_data_is_read_as_the_request_the_model_checks() {
    // What botocore sends for PutMetricData(Namespace="T", MetricData=[{MetricName="m", Timestamp=t, Value=2.5}]).
    let mut body = Vec::new();
    map_header(&mut body, 2);
    text(&mut body, "Namespace");
    text(&mut body, "T");
    text(&mut body, "MetricData");
    body.push(0x81);
    map_header(&mut body, 3);
    text(&mut body, "MetricName");
    text(&mut body, "m");
    text(&mut body, "Timestamp");
    body.push(0xc1);
    float(&mut body, NOW as f64 / 1000.0 - 5.0);
    text(&mut body, "Value");
    float(&mut body, 2.5);
    let json = aws_wire::cbor::to_json(&body).unwrap();
    let request: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut w = World::new();
    w.ok("PutMetricData", &request);
    let got = w.ok(
        "GetMetricStatistics",
        &json!({"Namespace": "T", "MetricName": "m", "Period": 60, "Statistics": ["Sum"],
            "StartTime": (NOW - 60_000) as f64 / 1000.0, "EndTime": (NOW + 60_000) as f64 / 1000.0}),
    );
    assert_eq!(got["Datapoints"][0]["Sum"], 2.5);
}

#[test]
fn an_answer_written_as_cbor_reads_back_as_the_same_json() {
    let mut w = World::new();
    w.put("m", NOW - 60_000, 7.0);
    let req = json!({"MetricDataQueries": [{"Id": "q", "MetricStat": {
        "Metric": {"Namespace": "T", "MetricName": "m"}, "Period": 60, "Stat": "Sum"}}],
        "StartTime": (NOW - 300_000) as f64 / 1000.0, "EndTime": (NOW + 300_000) as f64 / 1000.0});
    let ix = w.h.op("GetMetricData");
    let call = w.h.call(ix, &req).unwrap();
    let out = w.core.dispatch(&call).unwrap();
    let mut cbor = Vec::new();
    let shape = TABLES.op(ix).and_then(|o| o.output).unwrap();
    aws_schema::cbor::encode(&TABLES, shape, &out.arena, "", out.root, &mut cbor).unwrap();
    // The timestamps are tag 1 (0xc1) in the bytes, and numbers once read.
    assert!(cbor.contains(&0xc1));
    eprintln!("CBOR JSON: {}", aws_wire::cbor::to_json(&cbor).unwrap());
    let read: serde_json::Value =
        serde_json::from_str(&aws_wire::cbor::to_json(&cbor).unwrap()).unwrap();
    assert_eq!(norm(&read), norm(&w.h.json(ix, &out)));
}

/// Every number as a float, so that `9` and `9.0` compare equal.
fn norm(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Number(n) => json!(n.as_f64()),
        serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(norm).collect()),
        serde_json::Value::Object(m) => {
            serde_json::Value::Object(m.iter().map(|(k, x)| (k.clone(), norm(x))).collect())
        }
        other => other.clone(),
    }
}
