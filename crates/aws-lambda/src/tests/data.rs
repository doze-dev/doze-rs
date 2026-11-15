//! `doze-aws data` on Lambda: a function is measured by the code packages its `$LATEST` and published versions hold, a layer by its
//! versions' code; a function is never emptied (deleting it is the AWS call), but the code nothing points at and the runtime clients
//! go with `unused`; a reset removes every function, layer and mapping with all their code, and the names are free at once.

use std::sync::Arc;

use aws_core::base64;
use aws_wire::storage::{Prune, Resource, Storage, StorageError};
use doze_runtime::SystemClock;
use serde_json::json;

use super::{ACCOUNT, World, have};
use crate::core::Core;
use crate::data::{CLIENTS, UNUSED};

const PAD: usize = 256 << 10;

fn zip(handler: &str, pad: u8) -> String {
    let code = format!("def handler(e, c):\n    return {{'said': '{handler}'}}\n");
    base64::encode(&crate::zipread::build(&[
        ("app.py", code.as_bytes(), 0o644),
        ("pad.bin", &vec![pad; PAD], 0o644),
    ]))
}

fn create(name: &str, pad: u8) -> serde_json::Value {
    json!({"FunctionName": name, "Runtime": "python3.12", "Role": format!("arn:aws:iam::{ACCOUNT}:role/r"),
        "Handler": "app.handler", "Code": {"ZipFile": zip(name, pad)}, "Timeout": 10})
}

async fn usage(w: &World) -> Vec<Resource> {
    w.lambda.usage().await.unwrap().resources
}

fn find<'a>(all: &'a [Resource], name: &str) -> Option<&'a Resource> {
    all.iter().find(|r| r.name == name)
}

fn code_dirs(w: &World) -> usize {
    std::fs::read_dir(w.dir().join("lambda").join("code")).map_or(0, Iterator::count)
}

#[tokio::test]
async fn a_function_is_measured_by_its_code_and_only_what_nothing_uses_can_go() {
    let w = World::new();
    w.lambda.no_code_grace();
    w.ok("CreateFunction", create("app", b'a')).await;
    w.ok("PublishVersion", json!({"FunctionName": "app"})).await;
    w.ok(
        "UpdateFunctionCode",
        json!({"FunctionName": "app", "ZipFile": zip("app", b'b')}),
    )
    .await;
    w.ok(
        "PublishLayerVersion",
        json!({"LayerName": "deps", "Content": {"ZipFile": zip("deps", b'c')}}),
    )
    .await;
    // A create that is refused leaves the code it unpacked with nothing pointing at it.
    let again = w.err("CreateFunction", create("app", b'd')).await;
    assert_eq!(again.code, "ResourceConflictException");
    if have("python3") {
        let said = w.invoke("app", &json!({})).await;
        assert_eq!(said["Payload"]["said"], "app", "{said}");
    }

    let all = usage(&w).await;
    let app = find(&all, "app").unwrap();
    assert_eq!(app.items, 2, "$LATEST's code and version 1's");
    assert!(
        app.bytes > 4 * PAD as u64,
        "two archives and two unpacked trees: {app:?}"
    );
    assert_eq!(app.notes.len(), 1);
    assert!(
        app.notes[0].text.contains("published versions"),
        "{:?}",
        app.notes
    );
    assert!(app.notes[0].bytes > 2 * PAD as u64);
    let layer = find(&all, "layer:deps").unwrap();
    assert!(layer.bytes > 2 * PAD as u64, "{layer:?}");
    let unused = find(&all, UNUSED).unwrap();
    assert_eq!(unused.items, 1);
    assert!(unused.bytes > 2 * PAD as u64);
    let prune = unused.notes[0].prune.clone().unwrap();
    assert!(prune.unused);
    assert_eq!(
        find(&all, CLIENTS).is_some(),
        have("python3"),
        "the clients are there once a function ran"
    );

    // A function's code is not emptied; other prunes are refused by name; what is not there is not found.
    let refused = w.lambda.empty("us-east-1", "app", &Prune::default()).await;
    assert!(
        matches!(&refused, Err(StorageError::Unsupported(why)) if why.contains("DeleteFunction")),
        "{refused:?}"
    );
    let by_time = Prune {
        before_ms: Some(1),
        ..Prune::default()
    };
    assert!(matches!(
        w.lambda.empty("us-east-1", "app", &by_time).await,
        Err(StorageError::Unsupported(_))
    ));
    assert!(matches!(
        w.lambda
            .empty("us-east-1", "layer:deps", &Prune::default())
            .await,
        Err(StorageError::Unsupported(_))
    ));
    assert_eq!(
        w.lambda.empty("us-east-1", "nope", &Prune::default()).await,
        Err(StorageError::NotFound)
    );

    // A dry run counts and removes nothing; then the unused code goes and the functions are untouched.
    let dry = Prune {
        dry_run: true,
        ..prune.clone()
    };
    let would = w.lambda.empty("us-east-1", "app", &dry).await.unwrap();
    assert_eq!(would.freed, 0);
    assert_eq!(code_dirs(&w), 4);
    let done = w.lambda.empty("us-east-1", "app", &prune).await.unwrap();
    assert_eq!(done.items, would.items);
    assert!(done.freed > 2 * PAD as u64, "{done:?}");
    assert_eq!(code_dirs(&w), 3);
    let all = usage(&w).await;
    assert!(
        find(&all, UNUSED).is_none() && find(&all, CLIENTS).is_none(),
        "{all:?}"
    );
    assert_eq!(find(&all, "app").unwrap().bytes, app.bytes);
    if have("python3") {
        let said = w.invoke("app", &json!({})).await;
        assert_eq!(
            said["Payload"]["said"], "app",
            "the clients are written again: {said}"
        );
    }
    let v1 = w
        .ok(
            "GetFunction",
            json!({"FunctionName": "app", "Qualifier": "1"}),
        )
        .await;
    assert_eq!(v1["Configuration"]["Version"], "1");
    w.stop().await;
}

#[tokio::test]
async fn a_reset_removes_every_function_layer_and_mapping_with_their_code() {
    let w = World::new();
    w.ok("CreateFunction", create("app", b'a')).await;
    w.ok("PublishVersion", json!({"FunctionName": "app"})).await;
    w.ok(
        "PublishLayerVersion",
        json!({"LayerName": "deps", "Content": {"ZipFile": zip("deps", b'c')}}),
    )
    .await;
    w.peer("sqs", "CreateQueue", json!({"QueueName": "q"}))
        .await;
    let arn = format!("arn:aws:sqs:us-east-1:{ACCOUNT}:q");
    w.ok(
        "CreateEventSourceMapping",
        json!({"FunctionName": "app", "EventSourceArn": arn}),
    )
    .await;
    w.ok(
        "CreateFunctionUrlConfig",
        json!({"FunctionName": "app", "AuthType": "NONE"}),
    )
    .await;

    let dry = w.lambda.reset(true).await.unwrap();
    assert_eq!(dry.freed, 0);
    assert!(dry.items >= 3 && dry.bytes > 2 * PAD as u64, "{dry:?}");
    assert_eq!(code_dirs(&w), 2);

    let done = w.lambda.reset(false).await.unwrap();
    assert_eq!((done.items, done.bytes), (dry.items, dry.bytes));
    assert!(done.freed > 2 * PAD as u64, "{done:?}");
    assert_eq!(code_dirs(&w), 0);
    let functions = w.ok("ListFunctions", json!({})).await;
    assert_eq!(functions["Functions"], json!([]));
    let layers = w.ok("ListLayers", json!({})).await;
    assert_eq!(layers["Layers"], json!([]));
    let mappings = w.ok("ListEventSourceMappings", json!({})).await;
    assert_eq!(mappings["EventSourceMappings"], json!([]));
    assert!(usage(&w).await.is_empty());

    // The names are free at once, and a restart loads what the live service holds.
    w.ok("CreateFunction", create("app", b'e')).await;
    let dir = w.dir().join("lambda");
    w.tasks.shutdown().await;
    let core = Core::open_in(&dir, Arc::new(SystemClock), None).unwrap();
    assert_eq!(
        core.functions.iter().map(|(k, _)| k).collect::<Vec<_>>(),
        ["app"]
    );
    assert!(core.functions.get("app").unwrap().versions.is_empty());
    assert_eq!((core.layers.len(), core.mappings.len()), (0, 0));
}
