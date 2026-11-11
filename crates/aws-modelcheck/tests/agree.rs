//! The tables against the raw Smithy models, read here with no code shared with the generator: every operation is
//! there, and every structure in the tables has the members, the required flags and the kinds the model says.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aws_modelcheck::{Service, services};
use aws_schema::tables::{
    F_REQUIRED, K_BLOB, K_BOOL, K_EVENT, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT,
    K_TS, K_UNION,
};
use serde_json::Value;

fn model(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../models/{name}.json"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap()
}

fn local(id: &str) -> &str {
    id.split_once('#').map_or(id, |(_, n)| n)
}

fn has(traits: Option<&Value>, name: &str) -> bool {
    traits.is_some_and(|t| t.get(format!("smithy.api#{name}")).is_some())
}

/// Does the member have a default? A member's own `"default": null` means "none" and overrides the target's default.
fn defaulted(member: Option<&Value>, target: Option<&Value>) -> bool {
    let own = member.and_then(|t| t.get("smithy.api#default"));
    let from = own.or_else(|| target.and_then(|t| t.get("smithy.api#default")));
    from.is_some_and(|d| !d.is_null())
}

/// What kind the table should record for a model member that points at `target`.
fn expected_kind(shapes: &Value, target: &str) -> Option<u8> {
    let shape = shapes.get(target)?;
    let streaming = has(shape.get("traits"), "streaming");
    Some(match shape.get("type")?.as_str()? {
        "boolean" => K_BOOL,
        "byte" | "short" | "integer" | "long" => K_INT,
        "float" | "double" => K_FLOAT,
        "string" | "enum" => K_STR,
        "blob" if streaming => K_STREAM,
        "blob" => K_BLOB,
        "timestamp" => K_TS,
        "list" => K_LIST,
        "map" => K_MAP,
        "structure" => K_STRUCT,
        "union" if streaming => K_EVENT,
        "union" => K_UNION,
        _ => return None,
    })
}

/// The model's structure or union called `name` (the table may have suffixed a reserved name with `Shape`).
fn find_shape<'v>(shapes: &'v Value, ns_name: &str, table_name: &str) -> Option<(&'v Value, bool)> {
    let want = table_name.strip_suffix("Shape").unwrap_or(table_name);
    shapes.as_object()?.iter().find_map(|(id, s)| {
        let kind = s.get("type")?.as_str()?;
        (matches!(kind, "structure" | "union")
            && id.starts_with(ns_name)
            && (local(id) == table_name || local(id) == want))
            .then_some((s, kind == "union"))
    })
}

fn namespace(shapes: &Value) -> String {
    let service = shapes
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, s)| s.get("type").and_then(Value::as_str) == Some("service"));
    let id = service.map(|(id, _)| id.clone()).unwrap();
    id.split_once('#').unwrap().0.to_owned()
}

fn check_service(svc: &Service) -> (usize, usize) {
    let doc = model(svc.name);
    let shapes = &doc["shapes"];
    let ns = namespace(shapes);
    let t = svc.tables;
    let (mut checked, mut skipped) = (0, 0);
    for i in 0..t.n_shapes() {
        let sh = t.shape(i).unwrap();
        let Some((mshape, is_union)) = find_shape(shapes, &ns, sh.name) else {
            skipped += 1; // an overlay shape or a renamed one
            continue;
        };
        assert_eq!(sh.union, is_union, "{}: {} union", svc.name, sh.name);
        let members = mshape["members"].as_object().cloned().unwrap_or_default();
        let table: BTreeMap<&str, _> = (sh.first..sh.first + sh.count)
            .filter_map(|j| t.member(j))
            .map(|m| (m.name, m))
            .collect();
        let want: BTreeSet<&str> = members.keys().map(String::as_str).collect();
        assert_eq!(
            table.keys().copied().collect::<BTreeSet<_>>(),
            want,
            "{}: {} members",
            svc.name,
            sh.name
        );
        for (name, def) in &members {
            let m = &table[name.as_str()];
            let target = def["target"].as_str().unwrap();
            let traits = def.get("traits");
            if let Some(k) = expected_kind(shapes, target) {
                assert_eq!(m.kind, k, "{}: {}.{name} kind", svc.name, sh.name);
            }
            let target_traits = shapes.get(target).and_then(|s| s.get("traits"));
            let required =
                !is_union && has(traits, "required") && !defaulted(traits, target_traits);
            assert_eq!(
                m.flags & F_REQUIRED != 0,
                required,
                "{}: {}.{name} required",
                svc.name,
                sh.name
            );
        }
        checked += 1;
    }
    (checked, skipped)
}

#[test]
fn every_operation_of_the_model_is_in_the_tables() {
    for svc in services() {
        let doc = model(svc.name);
        let ns = namespace(&doc["shapes"]);
        let in_model: BTreeSet<String> = doc["shapes"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(id, s)| s["type"] == "operation" && id.starts_with(&ns))
            .map(|(id, _)| local(id).to_owned())
            .collect();
        let in_tables: BTreeSet<String> = (0..svc.tables.n_ops())
            .filter_map(|i| svc.tables.op(i))
            .map(|o| o.name.to_owned())
            .collect();
        assert_eq!(in_tables, in_model, "{}", svc.name);
    }
}

#[test]
fn structures_have_the_members_flags_and_kinds_the_model_says() {
    let (mut checked, mut skipped) = (0, 0);
    for svc in services() {
        let (c, s) = check_service(&svc);
        checked += c;
        skipped += s;
    }
    println!(
        "{checked} structures and unions checked against the models, {skipped} not matched by name"
    );
    // Every shape in the tables is a structure or union of the model; none is left unmatched by name.
    let total: usize = services()
        .iter()
        .map(|s| s.tables.n_shapes() as usize)
        .sum();
    assert_eq!((checked, skipped), (total, 0));
}
