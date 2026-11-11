//! A body built from the schema alone must be accepted, and each single fault in it must be rejected for the
//! right reason. Run over the input of every operation of the awsJson1_0 and awsJson1_1 services.
//!
//! The generator is deterministic and knows nothing the tables do not say, so a pass means the decoder agrees
//! with the tables; `agree.rs` is what ties the tables to the models.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

mod body;

use std::path::PathBuf;

use aws_modelcheck::{Service, services};
use aws_schema::arena::Arena;
use aws_schema::json::{Violation, decode};
use aws_schema::patterns::Patterns;
use aws_schema::tables::{F_REQUIRED, K_BOOL, K_INT, K_STR, MemberRef, RangeRef};
use serde_json::Value;

use body::{Gen, json};

fn kind(v: &Violation) -> &'static str {
    match v {
        Violation::Syntax { .. } => "Syntax",
        Violation::TooDeep => "TooDeep",
        Violation::Type { .. } => "Type",
        Violation::Missing { .. } => "Missing",
        Violation::Length { .. } => "Length",
        Violation::Range { .. } => "Range",
        Violation::Pattern { .. } => "Pattern",
        Violation::Enum { .. } => "Enum",
        Violation::Union { .. } => "Union",
        _ => "Table",
    }
}

fn is_json_service(name: &str) -> bool {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../models/{name}.json"));
    let doc: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    doc["shapes"].as_object().unwrap().values().any(|s| {
        s["type"] == "service"
            && (s["traits"].get("aws.protocols#awsJson1_0").is_some()
                || s["traits"].get("aws.protocols#awsJson1_1").is_some())
    })
}

/// One fault: the body to send and the kind of violation it must give.
type Fault = (String, &'static str, String);

/// Faults in the root object of `shape`, each built by replacing one member of the valid body `parts`.
fn faults(g: &Gen<'_>, shape: u32) -> Vec<Fault> {
    let sh = g.t.shape(shape).unwrap();
    if sh.union {
        return Vec::new();
    }
    let members: Vec<MemberRef<'_>> = (sh.first..sh.first + sh.count)
        .filter_map(|i| g.t.member(i))
        .collect();
    // The valid body as (wire name, json text) pairs, so a fault can swap one entry.
    let valid: Vec<(String, String)> = members
        .iter()
        .filter(|m| {
            m.flags & F_REQUIRED != 0
                && !matches!(
                    m.kind,
                    aws_schema::tables::K_STREAM | aws_schema::tables::K_EVENT
                )
        })
        .filter_map(|m| Some((m.wire.to_owned(), g.value(m, 1)?)))
        .collect();
    let body = |swap: Option<(&str, Option<&str>)>| {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in &valid {
            match swap {
                Some((sk, Some(nv))) if sk == k => parts.push(format!("{}:{nv}", json(k))),
                Some((sk, None)) if sk == k => {}
                _ => parts.push(format!("{}:{v}", json(k))),
            }
        }
        format!("{{{}}}", parts.join(","))
    };
    let mut out = Vec::new();
    for (k, _) in &valid {
        out.push((body(Some((k, None))), "Missing", format!("{k} left out")));
    }
    for m in &members {
        let Some(c) = g.cons(m) else { continue };
        let present = valid.iter().any(|(k, _)| k == m.wire);
        if m.kind == K_STR && present && c.enum_set != 0 {
            out.push((
                body(Some((m.wire, Some("\"~not-in-the-enum~\"")))),
                "Enum",
                format!("{} not in its enum", m.wire),
            ));
        }
        if m.kind == K_STR && present && c.enum_set == 0 && c.len_min.is_some_and(|n| n >= 1) {
            out.push((
                body(Some((m.wire, Some("\"\"")))),
                "Length",
                format!("{} empty", m.wire),
            ));
        }
        if m.kind == K_INT
            && present
            && let Some(RangeRef::Int(Some(lo), _)) = c.range
        {
            out.push((
                body(Some((m.wire, Some(&(lo - 1).to_string())))),
                "Range",
                format!("{} below {lo}", m.wire),
            ));
        }
        if matches!(m.kind, K_STR | K_BOOL) && present {
            let wrong = if m.kind == K_STR { "12" } else { "\"yes\"" };
            out.push((
                body(Some((m.wire, Some(wrong)))),
                "Type",
                format!("{} of the wrong type", m.wire),
            ));
        }
    }
    out
}

fn check_service(svc: &Service) -> (usize, usize, Vec<String>) {
    let t = svc.tables;
    let g = Gen {
        t,
        pats: Patterns::new(t),
        unmet: Default::default(),
    };
    let mut ar = Arena::new();
    let (mut tested, mut faulted, mut skipped) = (0, 0, Vec::new());
    for i in 0..t.n_ops() {
        let op = t.op(i).unwrap();
        let Some(input) = op.input else { continue };
        let Some(valid) = g.object(input, 0) else {
            skipped.push(format!("{}.{}", svc.name, op.name));
            continue;
        };
        ar.clear();
        let got = decode(t, &g.pats, input, &valid, &mut ar);
        assert!(
            got.is_ok(),
            "{}.{}: a valid body was refused: {got:?}\n{valid}",
            svc.name,
            op.name
        );
        tested += 1;
        for (body, want, what) in faults(&g, input) {
            ar.clear();
            let got = decode(t, &g.pats, input, &body, &mut ar);
            assert_eq!(
                got.as_ref().err().map(kind),
                Some(want),
                "{}.{}: {what}\n{body}",
                svc.name,
                op.name
            );
            faulted += 1;
        }
    }
    for p in g.unmet.borrow().iter() {
        println!("{}: no seed satisfies {p}", svc.name);
    }
    (tested, faulted, skipped)
}

#[test]
fn bodies_built_from_the_schema_are_accepted_and_each_fault_is_refused() {
    let (mut tested, mut faulted, mut skipped) = (0, 0, Vec::new());
    let mut services_run = 0;
    for svc in services().iter().filter(|s| is_json_service(s.name)) {
        let (t, f, s) = check_service(svc);
        tested += t;
        faulted += f;
        skipped.extend(s);
        services_run += 1;
    }
    println!(
        "{services_run} services, {tested} operations with a valid body, {faulted} faults refused, {} not generated",
        skipped.len()
    );
    println!("not generated: {skipped:?}");
    assert!(services_run >= 8, "{services_run} JSON services");
    assert!(tested > 400, "only {tested} operations were tested");
    assert!(
        skipped.len() * 10 < tested,
        "{} of {} operations could not be generated",
        skipped.len(),
        tested + skipped.len()
    );
}
