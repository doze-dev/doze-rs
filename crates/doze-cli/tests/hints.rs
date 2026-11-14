//! The module contract on the reading side: a module's `client` and `detect`, as doze-module-release writes them into
//! `meta.yaml`, come back whole from this machine's copy of the registry; a module that declares neither gets the
//! generic default (no shell, never guessed), and its label falls back to its name.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_cli::registry::Cache;

const VALKEY: &str = "title: Valkey
tagline: t
category: cache
engine: valkey
exampleLabel: cache
defaultVersion: \"9\"
client:
  program: valkey-cli
  args:
    - \"-h\"
    - \"{host}\"
  env:
    REDISCLI_AUTH: \"{password}\"
  fallbacks:
    - redis-cli
detect:
  packages:
    go:
      - github.com/redis/go-redis
    npm:
      - ioredis
  images:
    - valkey/valkey
  env:
    REDIS_URL: url
config:
  arguments: []
";

const BARE: &str = "title: Bare\ntagline: t\ncategory: c\nengine: bare\nconfig:\n  arguments: []\n";

#[test]
fn hints_round_trip_and_the_default_is_nothing() {
    let home = std::env::temp_dir().join(format!("dz-hints-{}", std::process::id()));
    for (m, text) in [("valkey", VALKEY), ("bare", BARE)] {
        let d = home.join("registry/doze").join(m);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("meta.yaml"), text).unwrap();
    }
    let hints = Cache::new(&home).hints("doze");
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(hints.len(), 2);
    let bare = &hints[0];
    assert_eq!(
        (bare.module.as_str(), bare.label.as_str()),
        ("bare", "bare")
    );
    assert!(bare.client.is_none() && bare.detect.is_none());
    let v = &hints[1];
    assert_eq!(v.label, "cache");
    let c = v.client.as_ref().unwrap();
    assert_eq!(
        (c.program.as_str(), c.args.as_slice()),
        ("valkey-cli", &["-h".to_owned(), "{host}".to_owned()][..])
    );
    assert_eq!(c.env["REDISCLI_AUTH"], "{password}");
    assert_eq!(c.fallbacks, ["redis-cli"]);
    let d = v.detect.as_ref().unwrap();
    assert_eq!(d.packages["go"], ["github.com/redis/go-redis"]);
    assert_eq!(d.images, ["valkey/valkey"]);
    assert_eq!(d.env["REDIS_URL"], "url");
}

/// The real round trip: what doze-module-release writes for a manifest is what the registry cache reads back.
#[test]
fn what_the_release_writes_is_what_doze_reads() {
    use doze_module_proto::msg::hello::Manifest;
    use doze_module_proto::msg::hints::{ClientCommand, Detect};
    use std::collections::BTreeMap;
    let client = ClientCommand {
        program: "psql".into(),
        args: vec!["{url}".into(), "-X".into()],
        env: BTreeMap::from([("PGPASSWORD".into(), "{password}".into())]),
        fallbacks: vec![],
    };
    let detect = Detect {
        packages: BTreeMap::from([
            ("go".into(), vec!["github.com/jackc/pgx".into()]),
            ("npm".into(), vec!["pg".into(), "@prisma/client".into()]),
        ]),
        images: vec!["postgres".into(), "postgis/postgis".into()],
        env: BTreeMap::from([("DATABASE_URL".into(), "url".into())]),
    };
    let m = Manifest {
        engine: "postgres".into(),
        category: "database".into(),
        title: "Postgres".into(),
        tagline: "t".into(),
        versions: vec!["17".into()],
        exact_dots: 0,
        default_port: 5432,
        boot_budget_ms: None,
        supervised: false,
        backend_socket: None,
        caps: 0,
        schema: Default::default(),
        example: String::new(),
        console: None,
        actions: vec![],
        targets: vec![],
        description: None,
        homepage: None,
        source: None,
        example_label: Some("db".into()),
        default_version: Some("17".into()),
        client: Some(client.clone()),
        detect: Some(detect.clone()),
    };
    let yaml = doze_module_release::meta_yaml(&m).unwrap();
    let home = std::env::temp_dir().join(format!("dz-hints-rt-{}", std::process::id()));
    let d = home.join("registry/doze/postgres");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("meta.yaml"), &yaml).unwrap();
    let got = Cache::new(&home).hints("doze");
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(got.len(), 1, "{yaml}");
    assert_eq!(got[0].label, "db");
    assert_eq!(got[0].client.as_ref(), Some(&client));
    assert_eq!(got[0].detect.as_ref(), Some(&detect));
}
