//! The echo module's console blocks (protocol minor 2), started as a real process and driven by a host peer: what it
//! declares, and each block's call answered through the SDK (a value in parts, a form's values checked, checks with a fix, a tree that pages and filters), and an older host's form call.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod echo_support;

use doze_module_proto::method::Method;
use doze_module_proto::msg::blocks::{
    Children, ChildrenArgs, ValueAnswer, ValueArgs, check, kind, media,
};
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::console::Resource;
use doze_module_proto::msg::error::{Error, ErrorKind};
use doze_module_proto::msg::hello::caps;
use doze_module_proto::msg::instance::Instance;
use doze_module_proto::msg::value::PathSeg;
use doze_module_proto::peer::{CallOpts, Peer};
use echo_support::{good, instance, start};
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;
use std::sync::Arc;

fn inst() -> Instance {
    instance("2", good(), "")
}

async fn value(p: &Peer, name: &str, offset: u64, max: u32) -> Result<ValueAnswer, Error> {
    let a = ValueArgs {
        instance: inst(),
        panel: "messages".into(),
        row: BTreeMap::from([("name".into(), name.into())]),
        offset,
        max,
    };
    p.call(Method::Value, &a, CallOpts::default()).await
}

async fn children(p: &Peer, parent: &str, filter: &str, cursor: Option<&str>) -> Children {
    let a = ChildrenArgs {
        instance: inst(),
        panel: "browse".into(),
        parent: parent.into(),
        filter: filter.into(),
        cursor: cursor.map(String::from),
    };
    p.call(Method::Children, &a, CallOpts::default())
        .await
        .unwrap()
}

async fn form(p: &Peer, action: &str, values: &[(&str, &str)]) -> Result<String, Error> {
    let a = RunAction {
        instance: inst(),
        action: action.into(),
        resource: String::new(),
        input: None,
        values: values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    };
    p.call(Method::RunAction, &a, CallOpts::default()).await
}

#[tokio::test(flavor = "current_thread")]
async fn the_manifest_declares_every_block() {
    let (_p, hello, _c) = start(Arc::default()).await;
    let m = hello.manifest;
    assert_eq!(
        m.caps & (caps::VALUE | caps::TREE),
        caps::VALUE | caps::TREE
    );
    let console = m.console.unwrap();
    let kinds: Vec<&str> = console
        .tabs
        .iter()
        .flat_map(|t| t.body.children.iter().map(|n| n.kind.as_str()))
        .collect();
    for k in [kind::VALUE, kind::SERIES, kind::CHECKS, kind::TREE] {
        assert!(kinds.contains(&k), "{k} in {kinds:?}");
    }
    let create = m.actions.iter().find(|a| a.id == "create").unwrap();
    let kinds: Vec<&str> = create.fields.iter().map(|f| f.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["text", "number", "choice", "bool", "pairs", "secret"]
    );
    assert_eq!(
        create.input_hint.as_deref(),
        Some("name"),
        "an older app's one box"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_value_comes_as_what_it_is_and_in_parts() {
    let (p, _h, _c) = start(Arc::default()).await;
    let json = value(&p, "order-10433", 0, 0).await.unwrap();
    assert_eq!(json.media, media::JSON);
    let parsed: serde_json::Value = serde_json::from_slice(&json.body).unwrap();
    assert_eq!(parsed["customer"]["name"], "Ada");
    assert_eq!(value(&p, "note", 0, 0).await.unwrap().media, media::TEXT);
    let bytes = value(&p, "blob", 0, 0).await.unwrap();
    assert_eq!((bytes.media.as_str(), bytes.body[1]), (media::BYTES, b'P'));
    let headers = value(&p, "headers", 0, 0).await.unwrap();
    assert_eq!(headers.media, media::PAIRS);
    assert_eq!(headers.pairs[1], ("source".to_string(), "api".to_string()));
    // In parts: 64 bytes, then the rest from where it stopped.
    let first = value(&p, "order-10433", 0, 64).await.unwrap();
    assert_eq!((first.body.len(), first.more), (64, true));
    let rest = value(&p, "order-10433", 64, 0).await.unwrap();
    assert!(!rest.more);
    assert_eq!(first.body.len() + rest.body.len(), first.size as usize);
    let e = value(&p, "nothing", 0, 0).await.unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
}

#[tokio::test(flavor = "current_thread")]
async fn a_form_is_checked_by_the_sdk_and_an_older_app_is_told_to_update() {
    let (p, _h, _c) = start(Arc::default()).await;
    let done = form(
        &p,
        "create",
        &[
            ("name", "payments"),
            ("size", "6"),
            ("headers", "a=1\nb=2"),
            ("token", "s3cret"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        done,
        "created channel payments (size 6, delete, kept in doze.hcl, 2 headers)"
    );
    assert!(!done.contains("s3cret"), "a secret is never echoed");
    let e = form(&p, "create", &[("name", "payments"), ("size", "0")])
        .await
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Invalid);
    assert_eq!(e.diagnostics[0].path, [PathSeg::Key("size".into())], "{e}");
    // An app that knows no forms sends the 2.1 shape: no values at all.
    #[derive(Encode, Decode)]
    struct RunAction21 {
        #[n(0)]
        instance: Instance,
        #[n(1)]
        action: String,
        #[n(2)]
        resource: String,
        #[n(3)]
        input: Option<String>,
    }
    let old = |action: &str| RunAction21 {
        instance: inst(),
        action: action.into(),
        resource: "a".into(),
        input: Some("payments".into()),
    };
    let e = p
        .call::<_, String>(Method::RunAction, &old("create"), CallOpts::default())
        .await
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.message.as_str()),
        (ErrorKind::Unsupported, doze_module::forms::NEEDS_NEWER_APP)
    );
    let purged: String = p
        .call(Method::RunAction, &old("purge"), CallOpts::default())
        .await
        .unwrap();
    assert_eq!(
        purged, "purged a",
        "an action without fields runs as before"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn checks_are_resources_with_a_fix_the_module_offers() {
    let (p, hello, _c) = start(Arc::default()).await;
    let res: Vec<Resource> = p
        .call(Method::Resources, &inst(), CallOpts::default())
        .await
        .unwrap();
    let checks: Vec<&Resource> = res.iter().filter(|r| r.kind == check::KIND).collect();
    let statuses: Vec<&str> = checks.iter().map(|c| c.status.as_str()).collect();
    assert_eq!(statuses, [check::OK, check::WARN, check::BAD]);
    let fix = &checks[2].info[check::FIX];
    assert!(
        hello.manifest.actions.iter().any(|a| &a.id == fix),
        "the fix {fix} is one of its actions"
    );
    assert_eq!(form(&p, fix, &[]).await.unwrap(), "applied 2 migrations");
    // The time chart's tables: a line per group, and connections against their limit.
    assert_eq!(res.iter().filter(|r| r.kind == "group").count(), 2);
    let server = res.iter().find(|r| r.kind == "server").unwrap();
    assert_eq!(server.info["max_connections"], "200");
}

#[tokio::test(flavor = "current_thread")]
async fn a_tree_opens_a_page_at_a_time_and_its_filter_searches_everything() {
    let (p, _h, _c) = start(Arc::default()).await;
    let top = children(&p, "", "", None).await;
    let ids: Vec<&str> = top.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!((ids, top.next), (vec!["app", "app_test"], None));
    let public = children(&p, "app/public", "", None).await;
    let orders = &public.nodes[0];
    assert!(orders.leaf);
    assert_eq!(orders.info["rows"], "12431");
    let table = value(&p, &orders.id, 0, 0).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&table.body).unwrap();
    assert_eq!(
        parsed["table"], "orders",
        "a leaf opens in the value viewer"
    );
    let mut seen = 0;
    let mut cursor = None;
    loop {
        let page = children(&p, "app/audit", "", cursor.as_deref()).await;
        assert!(page.nodes.len() <= 50);
        seen += page.nodes.len();
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, 120);
    let found = children(&p, "", "event-11", None).await;
    assert_eq!(found.nodes.len(), 10, "found in a folder never opened");
    let leaf = &found.nodes[0];
    let v = value(&p, &leaf.id, 0, 0).await.unwrap();
    assert_eq!(v.media, media::JSON);
}
