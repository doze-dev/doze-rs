//! Reading an import's objects from S3 and their formats: DynamoDB JSON lines, Ion, CSV.

use std::io::Read as _;
use std::sync::Arc;

use aws_expr::{Item, Number, Value, item_from_json};
use aws_peers::Peers;
use serde_json::{Value as Json, json};

use super::{Format, Job, MAX_OBJECT_BYTES, Read};
use crate::req::set;
use crate::work::Failure;

/// Reads every object under the prefix.
pub(crate) async fn read(peers: &Arc<Peers>, job: &Job) -> Result<Read, Failure> {
    let keys = list(peers, job).await?;
    if keys.is_empty() {
        return Err(Failure::new(
            "InvalidS3Source",
            format!(
                "No objects were found in bucket {} under the prefix '{}'",
                job.bucket, job.prefix
            ),
        ));
    }
    let mut out = Read::default();
    for key in keys {
        let raw = object(peers, job, &key).await?;
        out.bytes += raw.len() as u64;
        let text = match inflate(&raw, job.gzip) {
            Ok(t) => t,
            Err(_) => {
                out.errors += 1;
                continue;
            }
        };
        parse(&text, job, &mut out);
    }
    Ok(out)
}

async fn list(peers: &Arc<Peers>, job: &Job) -> Result<Vec<String>, Failure> {
    let mut out = Vec::new();
    let mut ask = json!({"Bucket": job.bucket, "Prefix": job.prefix});
    loop {
        let reply = peers
            .call_rest("dynamodb", &job.caller, "s3", "ListObjectsV2", &ask)
            .await
            .map_err(|e| crate::export::s3_failure(&job.bucket, &e))?;
        out.extend(
            reply
                .json
                .get("Contents")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|o| o.get("Key").and_then(Json::as_str))
                .filter(|k| !k.ends_with('/'))
                .map(str::to_owned),
        );
        match reply
            .json
            .get("NextContinuationToken")
            .and_then(Json::as_str)
        {
            Some(t) if reply.json.get("IsTruncated").and_then(Json::as_bool) == Some(true) => {
                set(&mut ask, "ContinuationToken", json!(t));
            }
            _ => return Ok(out),
        }
    }
}

async fn object(peers: &Arc<Peers>, job: &Job, key: &str) -> Result<Vec<u8>, Failure> {
    let mut reply = peers
        .call_rest(
            "dynamodb",
            &job.caller,
            "s3",
            "GetObject",
            &json!({"Bucket": job.bucket, "Key": key}),
        )
        .await
        .map_err(|e| crate::export::s3_failure(&job.bucket, &e))?;
    let mut bytes = Vec::new();
    if let Some(mut stream) = reply.body.take() {
        while let Some(chunk) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let chunk = chunk.map_err(|e| Failure::new("UNKNOWN", e.to_string()))?;
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > MAX_OBJECT_BYTES {
                return Err(Failure::new(
                    "UNKNOWN",
                    format!("The object {key} is larger than doze-aws reads (1 GiB)"),
                ));
            }
        }
    }
    Ok(bytes)
}

fn inflate(raw: &[u8], gzip: bool) -> Result<String, String> {
    if !gzip {
        return String::from_utf8(raw.to_vec()).map_err(|e| e.to_string());
    }
    let mut out = String::new();
    flate2::read::MultiGzDecoder::new(raw)
        .take(MAX_OBJECT_BYTES)
        .read_to_string(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Any text through every reader an import has (for the fuzz target): read or counted as an error, never a panic. Answers the
/// items read.
#[must_use]
pub fn check(text: &str) -> usize {
    let mut out = Read::default();
    let keys = [("pk".to_owned(), 'N'), ("b".to_owned(), 'B')];
    for format in [
        Format::Json,
        Format::Ion,
        Format::Csv {
            delimiter: ',',
            header: None,
        },
    ] {
        let job = Job {
            arn: String::new(),
            caller: crate::work::service_caller(""),
            bucket: String::new(),
            prefix: String::new(),
            format,
            gzip: false,
            keys: keys.to_vec(),
        };
        parse(text, &job, &mut out);
    }
    out.items.len()
}

fn parse(text: &str, job: &Job, out: &mut Read) {
    match &job.format {
        Format::Json => {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                match serde_json::from_str::<Json>(line)
                    .ok()
                    .and_then(|j| j.get("Item").map(item_from_json))
                {
                    Some(Ok(item)) => out.items.push(item),
                    _ => out.errors += 1,
                }
            }
        }
        Format::Ion => ion(text, out),
        Format::Csv { delimiter, header } => {
            csv(text, *delimiter, header.as_deref(), &job.keys, out)
        }
    }
}

/// Ion: each top-level struct (or each struct of a top-level list) holds an `Item`.
fn ion(text: &str, out: &mut Read) {
    let Ok(values) = crate::ion::read_all(text) else {
        out.errors += 1;
        return;
    };
    let mut each = |v: crate::ion::Ion| {
        let crate::ion::Ion::Struct(fields) = v else {
            out.errors += 1;
            return;
        };
        let item = fields
            .into_iter()
            .find(|(k, _)| k == "Item")
            .and_then(|(_, v)| crate::ion::to_value(v).ok());
        match item {
            Some(Value::M(m)) => out.items.push(m),
            _ => out.errors += 1,
        }
    };
    for v in values {
        match v {
            crate::ion::Ion::List(items, None) => items.into_iter().for_each(&mut each),
            other => each(other),
        }
    }
}

/// CSV: RFC 4180 records; the key attributes typed by their definitions, the rest strings, empty columns left out.
fn csv(
    text: &str,
    delimiter: char,
    header: Option<&[String]>,
    keys: &[(String, char)],
    out: &mut Read,
) {
    let mut records = records(text, delimiter).into_iter();
    let names: Vec<String> = match header {
        Some(h) => h.to_vec(),
        None => match records.next() {
            Some(first) => first,
            None => return,
        },
    };
    for fields in records {
        if fields.len() == 1 && fields.first().is_some_and(String::is_empty) {
            continue;
        }
        if fields.len() != names.len() {
            out.errors += 1;
            continue;
        }
        let mut item = Item::new();
        let mut ok = true;
        for (name, raw) in names.iter().zip(fields) {
            if raw.is_empty() {
                continue;
            }
            let ty = keys
                .iter()
                .find(|(k, _)| k == name)
                .map_or('S', |(_, t)| *t);
            let value = match ty {
                'N' => Number::parse(&raw).ok().map(Value::N),
                'B' => aws_core::base64::decode(&raw).map(Value::B),
                _ => Some(Value::S(raw)),
            };
            match value {
                Some(v) => {
                    item.insert(name.clone(), v);
                }
                None => ok = false,
            }
        }
        if ok {
            out.items.push(item);
        } else {
            out.errors += 1;
        }
    }
}

// fuzz:parser fuzz_import
/// Splits CSV text into records of fields: quotes hold delimiters and line breaks, `""` is a quote.
fn records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, c) if c == delimiter => record.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                record.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut record));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        out.push(record);
    }
    out
}

#[cfg(test)]
mod tests;
