//! The checks: majors, each target's privileges, and the names an instance may and may not have.

use super::*;

#[test]
fn majors_parse_like_go() {
    assert_eq!(major("17"), 17);
    assert_eq!(major("16.14"), 16);
    assert_eq!(major("x"), 0);
}

fn instance(name: &str) -> Instance {
    Instance {
        name: name.into(),
        engine: "postgres".into(),
        version: "18".into(),
        data_dir: "/tmp/dzo/d/x".into(),
        socket_dir: "/tmp/dzo/s/x".into(),
        spec: doze_module::proto::msg::value::Node::bare(
            doze_module::proto::msg::value::Value::Null,
        ),
        host: String::new(),
        listeners: Default::default(),
        deps: vec![],
        toolchain: None,
        dir: String::new(),
    }
}

fn messages(s: &PostgresSpec, name: &str) -> Vec<String> {
    check(s, &instance(name))
        .into_iter()
        .map(|d| d.message)
        .collect()
}

#[test]
fn an_instance_may_be_named_after_the_servers_own_database_but_not_a_template() {
    let mut s = crate::spec::tests::spec();
    s.owner = Some("identity".into());
    s.encoding = Some("utf8".into());
    s.schema.push(crate::spec::Schema {
        name: "identity".into(),
        owner: Some("identity".into()),
    });
    assert_eq!(messages(&s, "postgres"), Vec::<String>::new());
    for t in ["template0", "template1"] {
        assert!(messages(&s, t)[0].contains("every database"), "{t}");
    }
    // What initdb fixed, and closing the database doze works from, are refused at their line.
    s.encoding = Some("LATIN1".into());
    s.locale = Some("C".into());
    s.tablespace = Some("fast".into());
    s.allow_connections = false;
    let d = check(&s, &instance("postgres"));
    let paths: Vec<_> = d.iter().map(|d| d.path.clone()).collect();
    assert_eq!(
        paths,
        [
            vec![key("encoding")],
            vec![key("locale")],
            vec![key("tablespace")],
            vec![key("allow_connections")]
        ]
    );
    assert!(d.iter().all(|d| d.hint.is_some()));
    // Any other instance makes its database with them.
    assert_eq!(messages(&s, "app"), Vec::<String>::new());
}

#[test]
fn each_target_takes_its_own_privileges() {
    let db = privileges(&On::Database("d".into()), 17);
    assert!(db.contains(&"CONNECT") && !db.contains(&"SELECT"));
    assert!(privileges(&On::All("TABLES", "s".into()), 17).contains(&"MAINTAIN"));
    assert!(!privileges(&On::All("TABLES", "s".into()), 16).contains(&"MAINTAIN"));
}
