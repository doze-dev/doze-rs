//! A distributed Map's `ItemReader`: an S3 object read as a JSON array, JSON Lines or CSV (with its first row or the given headers as
//! the field names), or the objects of a bucket (`listObjectsV2`), at most `MaxItems` of them. A read that fails is
//! `States.ItemReaderFailed` (the interpreter names it).

use std::sync::Arc;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Map, Value, json};

fn fail(why: impl Into<String>) -> Failure {
    Failure::new(aws_asl::errors::ITEM_READER_FAILED, why)
}

pub(super) async fn read(
    peers: &Arc<Peers>,
    caller: &Caller,
    spec: &Value,
) -> Result<Value, Failure> {
    let resource = spec
        .get("Resource")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = spec.get("Parameters").cloned().unwrap_or_else(|| json!({}));
    let config = spec
        .get("ReaderConfig")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let most = config
        .get("MaxItems")
        .and_then(Value::as_u64)
        .map_or(usize::MAX, |n| usize::try_from(n).unwrap_or(usize::MAX));
    let mut items = match resource {
        "arn:aws:states:::s3:listObjectsV2" => list(peers, caller, &params).await?,
        "arn:aws:states:::s3:getObject" => {
            let text = object(peers, caller, &params).await?;
            parse(&text, &config)?
        }
        other => return Err(fail(format!("doze-aws cannot read items from {other}"))),
    };
    items.truncate(most);
    Ok(Value::Array(items))
}

async fn object(peers: &Arc<Peers>, caller: &Caller, params: &Value) -> Result<String, Failure> {
    let mut reply = peers
        .call_rest_as(caller, "s3", "GetObject", params, None)
        .await
        .map_err(|e| fail(format!("S3.{}: {}", e.code, e.message)))?;
    let mut bytes = Vec::new();
    if let Some(mut stream) = reply.body.take() {
        while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            bytes.extend_from_slice(&chunk.map_err(|e| fail(e.to_string()))?);
        }
    }
    String::from_utf8(bytes).map_err(|_| fail("The object is not UTF-8 text"))
}

async fn list(peers: &Arc<Peers>, caller: &Caller, params: &Value) -> Result<Vec<Value>, Failure> {
    let mut out = Vec::new();
    let mut ask = params.clone();
    loop {
        let reply = peers
            .call_rest_as(caller, "s3", "ListObjectsV2", &ask, None)
            .await
            .map_err(|e| fail(format!("S3.{}: {}", e.code, e.message)))?;
        out.extend(
            reply
                .json
                .get("Contents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        match reply
            .json
            .get("NextContinuationToken")
            .and_then(Value::as_str)
        {
            Some(t) if reply.json.get("IsTruncated").and_then(Value::as_bool) == Some(true) => {
                if let Some(m) = ask.as_object_mut() {
                    m.insert("ContinuationToken".into(), json!(t));
                }
            }
            _ => return Ok(out),
        }
    }
}

fn parse(text: &str, config: &Value) -> Result<Vec<Value>, Failure> {
    match config
        .get("InputType")
        .and_then(Value::as_str)
        .unwrap_or("JSON")
    {
        "JSON" => match serde_json::from_str::<Value>(text) {
            Ok(Value::Array(a)) => Ok(a),
            _ => Err(fail("The object is not a JSON array")),
        },
        "JSONL" => text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(|_| fail("A line of the object is not JSON")))
            .collect(),
        "CSV" => Ok(csv(text, config)),
        other => Err(fail(format!(
            "The InputType {other} is not read by doze-aws"
        ))),
    }
}

/// CSV rows as objects of strings. Quoted fields may hold commas and doubled quotes.
fn csv(text: &str, config: &Value) -> Vec<Value> {
    let mut rows = text.lines().filter(|l| !l.is_empty()).map(fields);
    let headers: Vec<String> = match config.get("CSVHeaderLocation").and_then(Value::as_str) {
        Some("GIVEN") => config
            .get("CSVHeaders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|h| h.as_str().map(str::to_owned))
            .collect(),
        _ => rows.next().unwrap_or_default(),
    };
    rows.map(|r| {
        Value::Object(
            headers
                .iter()
                .zip(r)
                .map(|(h, v)| (h.clone(), Value::String(v)))
                .collect::<Map<_, _>>(),
        )
    })
    .collect()
}

fn fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn readers() {
        assert_eq!(
            parse("[1,2]", &json!({})).unwrap(),
            vec![json!(1), json!(2)]
        );
        assert_eq!(
            parse("{\"a\":1}\n{\"a\":2}\n", &json!({"InputType": "JSONL"}))
                .unwrap()
                .len(),
            2
        );
        let rows = parse("name,qty\n\"a, b\",2\nc,3", &json!({"InputType": "CSV"})).unwrap();
        assert_eq!(rows[0], json!({"name": "a, b", "qty": "2"}));
        let given = parse(
            "x,1",
            &json!({"InputType": "CSV", "CSVHeaderLocation": "GIVEN", "CSVHeaders": ["k", "v"]}),
        )
        .unwrap();
        assert_eq!(given, vec![json!({"k": "x", "v": "1"})]);
    }
}
