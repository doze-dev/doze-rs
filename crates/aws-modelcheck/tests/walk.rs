//! The tables are read with `get`-and-default accessors (a bad index reads as "none", it never panics), which is only
//! sound if the generator never writes a bad index. This walks every index in every table of every model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use aws_modelcheck::services;
use aws_schema::tables::{
    K_BLOB, K_BOOL, K_EVENT, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STREAM, K_STRUCT, K_TS,
    K_UNION, Tables,
};

fn walk_members(name: &str, t: &Tables) {
    for i in 0..t.n_members() {
        let m = t
            .member(i)
            .unwrap_or_else(|| panic!("{name}: member {i} unreadable"));
        assert!(!m.name.is_empty(), "{name}: member {i} has no name");
        assert!(m.wire.len() < 4096, "{name}: member {i}");
        assert!(
            m.cons < t.n_cons(),
            "{name}: member {i} ({}) constraint {} of {}",
            m.name,
            m.cons,
            t.n_cons()
        );
        match m.kind {
            K_BOOL | K_INT | K_FLOAT | K_STR | K_BLOB | K_STREAM | K_EVENT | K_TS => {}
            K_STRUCT | K_UNION => assert!(
                m.target < t.n_shapes(),
                "{name}: member {i} ({}) shape {}",
                m.name,
                m.target
            ),
            K_LIST => assert!(
                t.member(m.target).is_some(),
                "{name}: list member {i} element {}",
                m.target
            ),
            K_MAP => assert!(
                t.member(m.target).is_some() && t.member(m.target + 1).is_some(),
                "{name}: map member {i} key/value {}",
                m.target
            ),
            other => panic!("{name}: member {i} has kind {other}"),
        }
    }
}

fn walk_constraints(name: &str, t: &Tables) {
    for i in 0..t.n_cons() {
        let c = t
            .cons(i)
            .unwrap_or_else(|| panic!("{name}: constraint {i} unreadable"));
        assert!(
            c.enum_set < t.n_enum_sets(),
            "{name}: constraint {i} enum set {}",
            c.enum_set
        );
        if let (Some(a), Some(b)) = (c.len_min, c.len_max) {
            assert!(a <= b, "{name}: constraint {i} length {a}..{b}");
        }
    }
    for set in 1..t.n_enum_sets() {
        assert!(
            !t.enum_values(set).is_empty(),
            "{name}: enum set {set} is empty"
        );
    }
}

fn walk_shapes_and_ops(name: &str, t: &Tables) {
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..t.n_shapes() {
        let s = t
            .shape(i)
            .unwrap_or_else(|| panic!("{name}: shape {i} unreadable"));
        assert!(!s.name.is_empty(), "{name}: shape {i} has no name");
        assert!(
            seen.insert(s.name),
            "{name}: two shapes are called {}",
            s.name
        );
        assert!(
            s.first + s.count <= t.n_members(),
            "{name}: shape {} members {}+{}",
            s.name,
            s.first,
            s.count
        );
    }
    let mut ops = std::collections::BTreeSet::new();
    for i in 0..t.n_ops() {
        let op = t
            .op(i)
            .unwrap_or_else(|| panic!("{name}: operation {i} unreadable"));
        assert!(
            ops.insert(op.name),
            "{name}: two operations are called {}",
            op.name
        );
        for shape in [op.input, op.output].into_iter().flatten() {
            assert!(shape < t.n_shapes(), "{name}: {} shape {shape}", op.name);
        }
        for e in t.op_errors(&op) {
            assert!(
                e.shape < t.n_shapes(),
                "{name}: {} error shape {}",
                op.name,
                e.shape
            );
            assert!(
                !e.code.is_empty() && (400..600).contains(&e.status),
                "{name}: {} error {} {}",
                op.name,
                e.code,
                e.status
            );
        }
        assert_eq!(
            t.op_errors(&op).count(),
            op.errors.1 as usize,
            "{name}: {} lost an error",
            op.name
        );
    }
}

#[test]
fn every_index_in_every_table_is_in_range() {
    for s in services() {
        walk_members(s.name, s.tables);
        walk_constraints(s.name, s.tables);
        walk_shapes_and_ops(s.name, s.tables);
    }
}

#[test]
fn the_models_hold_every_operation_aws_defines() {
    let all = services();
    for s in &all {
        assert_eq!(
            s.operations,
            s.tables.n_ops() as usize,
            "{}: Operation::ALL against the tables",
            s.name
        );
    }
    // S1 counted 1,372 operations over the 18 models; a change here is a model change and should be read.
    assert_eq!(all.iter().map(|s| s.operations).sum::<usize>(), 1372);
}
