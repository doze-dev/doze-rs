use super::{one_line, scan, shorten};

const ACME: &str = include_str!("../../../testdata/go/hcl/acme.hcl");
const LARDER: &str = include_str!("../../../testdata/go/hcl/larder.hcl");

#[test]
fn reads_the_blocks_of_a_stack() {
    let f = scan(ACME);
    assert_eq!(f.stack, "acme");
    let names: Vec<(&str, &str)> = f
        .blocks
        .iter()
        .map(|b| (b.kind.as_str(), b.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("app", "api"),
            ("app", "web"),
            ("app", "worker"),
            ("module", "db"),
            ("module", "cache"),
            ("task", "migrate")
        ]
    );
    let api = &f.blocks[0];
    assert_eq!(api.run, "go run ./cmd/api");
    assert_eq!(api.port, Some(8080));
    assert_eq!(api.needs, ["db", "cache"]);
    let web = &f.blocks[1];
    assert_eq!(web.domains, ["dev.acme.com"]);
    assert_eq!(web.needs, ["api"]);
    let db = &f.blocks[3];
    assert_eq!((db.ty.as_str(), db.version.as_str()), ("postgres", "17"));
    assert_eq!(f.refs["api"]["db"], "env = …module.db.url…");
}

#[test]
fn quotes_the_file_for_the_grid() {
    let f = scan(LARDER);
    assert_eq!(f.refs["orders"]["catalog"], "CATALOG_URL = app.catalog.url");
    assert_eq!(
        f.refs["storefront"]["after"],
        "after = [task.install-storefront]"
    );
    assert_eq!(
        f.refs["storefront"]["install-storefront"],
        "after = [task.install-storefront]"
    );
    assert!(
        f.blocks
            .iter()
            .any(|b| b.name == "postgres" && b.port == Some(5432))
    );
}

#[test]
fn lines_read_as_a_tooltip_shows_them() {
    assert_eq!(
        one_line("  CATALOG_URL   = app.catalog.url,   # the shelves"),
        "CATALOG_URL = app.catalog.url"
    );
    assert_eq!(
        one_line("after = [ task.a, task.b ]"),
        "after = [task.a, task.b]"
    );
    assert_eq!(one_line(r#"X = "a #b" # c"#), r#"X = "a #b" # c"#);
}

#[test]
fn long_lines_keep_the_reference() {
    let long = r#"DATABASE_URL = "postgres://orders:orders@${module.postgres.address}/larder?search_path=orders&sslmode=disable""#;
    assert_eq!(
        shorten(long, "module.postgres"),
        r#"DATABASE_URL = "…${module.postgres.address}…""#
    );
    assert_eq!(shorten("short = app.x", "app.x"), "short = app.x");
}

#[test]
fn nothing_breaks_it() {
    for src in [
        "",
        "{",
        "}",
        "app \"x\" {",
        "a = \"${",
        "x = <<EOT\n${app.a",
        "app { app {",
        "\"\\",
        "é = app.é.b",
    ] {
        let _ = scan(src);
    }
    let deep = "a {".repeat(200);
    let _ = scan(&deep);
}
