#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;

fn ok(text: &str) -> Statement {
    parse(text)
        .unwrap_or_else(|e| panic!("{text}: {e}"))
        .statement
}

fn name(n: &str) -> Path {
    Path(vec![Step::Name(n.to_owned())])
}

#[test]
fn the_four_statements_read() {
    assert_eq!(
        ok(r#"SELECT * FROM "Music" WHERE Artist = ? AND "SongTitle" = 'Hi'"#),
        Statement::Select {
            table: "Music".into(),
            index: None,
            columns: None,
            filter: Some(Expr::And(
                Box::new(Expr::Cmp(
                    Operand::Path(name("Artist")),
                    Cmp::Eq,
                    Operand::Value(Lit::Param(0))
                )),
                Box::new(Expr::Cmp(
                    Operand::Path(name("SongTitle")),
                    Cmp::Eq,
                    Operand::Value(Lit::Str("Hi".into()))
                )),
            )),
        }
    );
    assert!(matches!(
        ok(r#"select a, b.c[2] from t."by-x" where x between 1 and 2"#),
        Statement::Select { index: Some(i), columns: Some(c), .. } if i == "by-x" && c.len() == 2
    ));
    assert_eq!(
        ok(
            "INSERT INTO t VALUE {'pk': ?, 'n': -1.5e3, 'l': [1, 'x'], 's': <<'a','b'>>, 'm': {}, 'b': true, 'z': null}"
        ),
        Statement::Insert {
            table: "t".into(),
            item: vec![
                ("pk".into(), Lit::Param(0)),
                ("n".into(), Lit::Num("-1.5e3".into())),
                (
                    "l".into(),
                    Lit::List(vec![Lit::Num("1".into()), Lit::Str("x".into())])
                ),
                (
                    "s".into(),
                    Lit::Bag(vec![Lit::Str("a".into()), Lit::Str("b".into())])
                ),
                ("m".into(), Lit::Map(vec![])),
                ("b".into(), Lit::Bool(true)),
                ("z".into(), Lit::Null),
            ],
        }
    );
    let Statement::Update {
        set,
        remove,
        returning,
        ..
    } = ok(
        "UPDATE t SET a = a + 1, b = ? SET c = list_append(c, [1]) REMOVE d, e[0] WHERE pk = 'x' RETURNING ALL NEW *",
    )
    else {
        panic!()
    };
    assert_eq!(
        (set.len(), remove.len(), returning),
        (3, 2, Some(Returning::AllNew))
    );
    assert!(matches!(
        ok("DELETE FROM t WHERE pk = ? AND NOT (x IS MISSING);"),
        Statement::Delete { .. }
    ));
}

#[test]
fn parameters_are_counted_in_order() {
    let p = parse("UPDATE t SET a = ? WHERE pk = ? AND sk IN [?, ?]").unwrap();
    assert_eq!(p.params, 4);
}

#[test]
fn what_is_not_built_is_refused_by_name() {
    for (text, what) in [
        ("SELECT * FROM t ORDER BY a", "ORDER BY"),
        ("SELECT COUNT(*) FROM t", "functions in a SELECT list"),
        ("SELECT * FROM t WHERE EXISTS (SELECT * FROM u)", "EXISTS"),
        ("UPSERT INTO t VALUE {}", "UPSERT and REPLACE"),
        (
            "UPDATE t SET a = set_add(b, <<1>>) WHERE pk = 1",
            "set_add and set_delete on an attribute other than the one assigned",
        ),
    ] {
        assert_eq!(parse(text).unwrap_err(), Error::NotBuilt(what), "{text}");
    }
}

#[test]
fn malformed_statements_are_refused() {
    for text in [
        "",
        "SELECT",
        "SELECT * FROM",
        "SELECT * FROM t WHERE",
        "SELECT * FROM t WHERE a",
        "SELECT * FROM t WHERE a = 'x",
        "INSERT INTO t VALUE 1",
        "INSERT INTO t VALUE {pk: 1}",
        "UPDATE t WHERE pk = 1",
        "DELETE FROM t",
        "DELETE FROM t WHERE pk = 1 RETURNING ALL NEW *",
        "SELECT * FROM t; SELECT * FROM t",
        "SELECT * FROM t WHERE a[x] = 1",
        "SELECT * FROM t WHERE a = <<>>",
        "SELECT * FROM t WHERE a = 1 #",
    ] {
        assert!(matches!(parse(text), Err(Error::Syntax(_))), "{text}");
    }
}

#[test]
fn nesting_and_length_are_bounded() {
    let deep = format!(
        "SELECT * FROM t WHERE {}a = 1{}",
        "(".repeat(200),
        ")".repeat(200)
    );
    assert!(matches!(parse(&deep), Err(Error::Syntax(_))));
    let nots = format!("SELECT * FROM t WHERE {}a = 1", "NOT ".repeat(200));
    assert!(matches!(parse(&nots), Err(Error::Syntax(_))));
    let lists = format!(
        "INSERT INTO t VALUE {{'a': {}1{}}}",
        "[".repeat(200),
        "]".repeat(200)
    );
    assert!(matches!(parse(&lists), Err(Error::Syntax(_))));
    let long = format!("SELECT * FROM t WHERE a = '{}'", "x".repeat(9000));
    assert!(matches!(parse(&long), Err(Error::Syntax(_))));
}

/// The fuzz target's property, run on a fixed stream of mutations so it is part of every test run: any text is read or
/// refused, never a panic.
#[test]
fn mutated_statements_never_panic() {
    let seeds = [
        "SELECT * FROM \"t\".\"i\" WHERE a = ? AND (b <> 'x' OR c BETWEEN 1 AND 2) AND d IN [1,2] AND begins_with(e, 'p')",
        "INSERT INTO t VALUE {'a': [1, {'b': <<'c'>>}], 'd': -2.5e-3}",
        "UPDATE t SET a = if_not_exists(a, 0) + 1 REMOVE b[3].c WHERE pk = ? RETURNING MODIFIED OLD *",
        "DELETE FROM t WHERE pk = 'x' AND attribute_type(a, 'S') AND size(b) > 3 AND c IS NOT NULL",
    ];
    let alphabet: Vec<char> =
        "SELECT*FROMWHEREAND OR NOT()[]{}<>=!?'\".,:-+;0123456789eabc_\u{e9}\n"
            .chars()
            .collect();
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let mut rand = |n: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % (n as u64)).unwrap_or(0)
    };
    for _ in 0..20_000 {
        let mut chars: Vec<char> = seeds[rand(seeds.len())].chars().collect();
        for _ in 0..1 + rand(6) {
            let at = rand(chars.len() + 1);
            match rand(3) {
                0 if at < chars.len() => {
                    chars.remove(at);
                }
                1 => chars.insert(at, alphabet[rand(alphabet.len())]),
                _ if at < chars.len() => chars[at] = alphabet[rand(alphabet.len())],
                _ => {}
            }
        }
        let text: String = chars.into_iter().collect();
        let _ = parse(&text);
    }
}
