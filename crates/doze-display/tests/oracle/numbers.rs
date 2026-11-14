//! The figure cases: sizes, counts, spans, the scale labels, and the series arithmetic.

use std::time::Duration;

use doze_display as d;
use doze_display::Share;
use serde_json::{Value, json};

use crate::{bits, nanos, now};

fn float(v: &Value) -> f64 {
    f64::from_bits(v.as_u64().expect("float bits"))
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array().expect("an array").iter().map(float).collect()
}

fn all_bits(vs: &[f64]) -> Vec<u64> {
    vs.iter().copied().map(bits).collect()
}

/// `int64(v)` is the same on every platform only for a number in range; the oracle leaves the rest out.
fn fits(v: f64) -> bool {
    v > -9.2e18 && v < 9.2e18
}

pub(crate) fn say(case: &str, input: &Value) -> Value {
    match case {
        "int" => {
            let n = input.as_i64().expect("an integer");
            json!({"mb": d::mb(n), "thousands": d::thousands(n), "plural": d::plural(u64::try_from(n).unwrap_or(0))})
        }
        "duration" => {
            let span = Duration::from_nanos(input.as_u64().expect("nanoseconds"));
            json!({"clock": d::clock(span), "ago": d::ago(span)})
        }
        "float" => {
            let (v, total) = (float(&input[0]), float(&input[1]));
            let sized = |f: fn(f64) -> String| if fits(v) { json!(f(v)) } else { Value::Null };
            json!({
                "cores": d::cores(v), "cores_unit": d::cores_unit(v), "pct": d::pct(v), "count": d::count(v),
                "bytes_label": sized(d::bytes_label), "mem_unit": sized(d::mem_unit(total)),
            })
        }
        "span" => json!(d::span_word(input.as_u64().expect("a count"))),
        "series" => series(input),
        other => panic!("not a figure case: {other}"),
    }
}

fn series(input: &Value) -> Value {
    let vs = floats(&input["vs"]);
    let floor = float(&input["floor"]);
    let w = usize::try_from(input["w"].as_i64().expect("a width")).unwrap_or(0);
    let zoom = input["zoom"].as_bool().expect("a flag");
    let from = input["times_from"].as_u64().expect("an offset");
    let times: Vec<_> = (from..vs.len() as u64)
        .map(|i| now() + Duration::from_secs(i))
        .collect();
    let (peak, mean) = d::stats(&vs);
    let (lo, hi) = d::scale(&vs, floor, zoom);
    let (sv, st) = d::squeeze_timed(&vs, &times, w);
    let stamps: Vec<i64> = st.iter().copied().map(nanos).collect();
    let names: Vec<&str> = input["names"]
        .as_array()
        .expect("names")
        .iter()
        .map(|n| n.as_str().expect("a name"))
        .collect();
    let all = names
        .iter()
        .zip(&vs)
        .map(|(name, v)| Share { name, v: *v })
        .collect();
    let m = d::shares(all, floor);
    let drawn: Vec<Value> = m
        .shares
        .iter()
        .map(|s| json!([s.name, bits(s.v)]))
        .collect();
    let inner = usize::try_from(input["inner"].as_u64().expect("a width")).expect("fits");
    let finite = m.sum.is_finite() && m.shares.iter().all(|s| s.v.is_finite());
    let cells = if finite {
        json!(d::share_cells(&m.shares, m.sum, inner))
    } else {
        Value::Null
    };
    json!({
        "stats": all_bits(&[peak, mean]), "scale": all_bits(&[lo, hi]), "squeeze": all_bits(&d::squeeze(&vs, w)),
        "squeeze_timed": [all_bits(&sv), stamps], "shares": drawn, "idle": m.idle, "sum": bits(m.sum), "cells": cells,
    })
}
