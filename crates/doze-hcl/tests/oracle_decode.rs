//! Bodies against a schema, against Go's hcl (`oracle-dump content`): `Content`, `PartialContent`
//! (and the rest of the body), `JustAttributes`, merged files, then each block's body: the names
//! found, the blocks, the missing-item range and every diagnostic with its range and detail.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use doze_hcl::decode::{BodyRef, Schema};
use doze_hcl::diag::{Diagnostics, Range};
use doze_hcl::syntax::{Source, parse_body};
use serde_json::{Value as Json, json};

fn rng(file: &str, r: Range) -> Json {
    json!({"file": file, "start": {"line": r.start.line, "col": r.start.column, "byte": r.start.byte},
           "end": {"line": r.end.line, "col": r.end.column, "byte": r.end.byte}})
}

fn diags(file: &str, ds: &Diagnostics) -> Json {
    let _ = file;
    let mut out: Vec<Json> = ds
        .iter()
        .map(|d| {
            let mut m = json!({"sev": "error", "summary": d.summary, "detail": d.detail});
            if let Some(r) = d.subject {
                m["at"] = rng(d.file.as_deref().unwrap_or("f0.hcl"), r);
            }
            if let Some(r) = d.context {
                m["context"] = rng(d.file.as_deref().unwrap_or("f0.hcl"), r);
            }
            m
        })
        .collect();
    // Go sorts by its JSON of the range: keys end, file, start; each position line, col, byte.
    let pos = |p: &Json| {
        format!(
            "{{\"line\":{},\"col\":{},\"byte\":{}}}",
            p["line"], p["col"], p["byte"]
        )
    };
    let key = |d: &Json| {
        let at = d.get("at").map_or("null".to_owned(), |a| {
            format!(
                "{{\"end\":{},\"file\":{},\"start\":{}}}",
                pos(&a["end"]),
                a["file"],
                pos(&a["start"])
            )
        });
        format!(
            "{at}{}{}",
            d["summary"].as_str().unwrap(),
            d["detail"].as_str().unwrap()
        )
    };
    out.sort_by_key(key);
    Json::Array(out)
}

fn read(body: &BodyRef<'_>, mode: &str, schema: &Schema, nest: bool) -> Json {
    let file = "f0.hcl";
    let mut out = json!({});
    let (attrs, blocks, ds) = match mode {
        "just" => {
            let (a, d) = body.just_attributes();
            (a.keys().cloned().collect::<Vec<_>>(), Vec::new(), d)
        }
        "partial" => {
            let (c, rest, d) = body.partial_content(schema);
            let (ra, rd) = rest.just_attributes();
            out["rest"] = json!(ra.keys().cloned().collect::<Vec<_>>());
            out["rest_diags"] = diags(file, &rd);
            (c.attributes.keys().cloned().collect(), c.blocks, d)
        }
        _ => {
            let (c, d) = body.content(schema);
            (c.attributes.keys().cloned().collect(), c.blocks, d)
        }
    };
    out["attrs"] = json!(attrs);
    out["missing"] = rng(body.file_name(), body.missing_item_range());
    out["blocks"] = Json::Array(
        blocks
            .iter()
            .map(|b| {
                let mut e = json!({"type": b.ty, "labels": if b.labels.is_empty() { Json::Null } else { json!(b.labels) }});
                if nest {
                    e["body"] = read(&b.body, "content", schema, false);
                }
                e
            })
            .collect(),
    );
    out["diags"] = diags(file, &ds);
    out
}

fn schema(c: &Json) -> Schema {
    let mut s = Schema::default();
    for a in c["attrs"].as_array().unwrap() {
        s = s.attr(a[0].as_str().unwrap(), a[1].as_bool().unwrap());
    }
    for b in c["blocks"].as_array().unwrap() {
        let labels: Vec<&str> = b[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap())
            .collect();
        s = s.block(b[0].as_str().unwrap(), &labels);
    }
    s
}

#[test]
fn bodies_read_against_a_schema_as_go_reads_them() {
    for (set, go) in [("cases", "go"), ("cases-audit", "go-audit")] {
        check_set(set, go);
    }
}

fn check_set(set: &str, go_set: &str) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/decode");
    let cases: Vec<Json> =
        serde_json::from_str(&std::fs::read_to_string(format!("{dir}/{set}.json")).unwrap())
            .unwrap();
    let go: Vec<Json> = std::fs::read_to_string(format!("{dir}/{go_set}.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        let texts: Vec<&str> = c["srcs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        let srcs: Vec<Source> = texts.iter().map(|t| Source::new(t)).collect();
        let bodies: Vec<_> = srcs.iter().map(parse_body).collect();
        if bodies.iter().any(Result::is_err) {
            if g["parse"] != json!(false) {
                diffs.push(format!("{}: refused, Go parsed", c["id"]));
            }
            continue;
        }
        let names: Vec<String> = (0..srcs.len()).map(|i| format!("f{i}.hcl")).collect();
        let refs: Vec<BodyRef<'_>> = bodies
            .iter()
            .zip(&srcs)
            .zip(&names)
            .map(|((b, s), n)| BodyRef::file(b.as_ref().unwrap(), s, n))
            .collect();
        let body = if refs.len() == 1 {
            refs.into_iter().next().unwrap()
        } else {
            BodyRef::merge(refs)
        };
        let got = read(&body, c["mode"].as_str().unwrap(), &schema(c), true);
        // Ranges outside diagnostics (the missing-item range) carry no file here.
        // Keys sorted, whether or not a workspace build turns on serde_json's `preserve_order`.
        let strip = |j: &Json| sorted(j).to_string();
        if strip(&got) != strip(&g["top"]) {
            diffs.push(format!(
                "{} {}: {:?}\n   go:   {}\n   rust: {}",
                c["id"], c["mode"], texts, g["top"], got
            ));
        }
    }
    eprintln!(
        "decode {set}: {} of {} identical",
        cases.len() - diffs.len(),
        cases.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differ:\n{}",
        diffs.len(),
        diffs.iter().take(6).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The same value with every object's keys in order.
fn sorted(j: &Json) -> Json {
    match j {
        Json::Object(o) => {
            let mut keys: Vec<_> = o.iter().collect();
            keys.sort_by(|a, b| a.0.cmp(b.0));
            Json::Object(
                keys.into_iter()
                    .map(|(k, v)| (k.clone(), sorted(v)))
                    .collect(),
            )
        }
        Json::Array(a) => Json::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}
