//! The registry over the broker's metadata database: what it holds is found again after the database is closed and
//! opened, on the simulated disk the broker's own records are crash-tested on.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::path::Path;
use std::sync::Arc;

use kafka_meta::MetaStore;
use kafka_registry::format::SchemaType;
use kafka_registry::registry::{Level, Origin, RegisterResult, Registry, SchemaInput};
use kafka_registry::store::MetaStorage;
use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;

#[test]
fn the_registry_survives_a_restart_of_the_metadata_database() {
    let fs = SimFs::new();
    let dir = Path::new("/data");
    fs.create_dir(dir).unwrap();
    fs.sync_dir(Path::new("/")).unwrap();
    let schema = r#"{"type":"record","name":"R","fields":[{"name":"a","type":"string"}]}"#;
    {
        let meta = Arc::new(MetaStore::open(&fs, dir).unwrap());
        let r = Registry::open(Box::new(MetaStorage(Arc::clone(&meta)))).unwrap();
        let got = r.register(
            "orders-value",
            &SchemaInput::of(SchemaType::Avro, schema),
            false,
            &Origin::default(),
        );
        assert_eq!(got.unwrap(), RegisterResult::Id(1));
        r.set_level("orders-value", "FULL").unwrap();
    }
    let meta = Arc::new(MetaStore::open(&fs, dir).unwrap());
    let r = Registry::open(Box::new(MetaStorage(meta))).unwrap();
    assert_eq!(r.subjects("", false, false), ["orders-value"]);
    assert_eq!(
        r.version("orders-value", "latest", false).unwrap().schema,
        schema
    );
    assert_eq!(r.level("orders-value", false).unwrap(), Level::Full);
    assert_eq!(r.max_id(), 1);
}
