//! The key browser and the set-key form, checked on a real engine through the module's own calls. Valkey's and
//! kvrocks's acceptance tests both include this file (`#[path]`), since the browser is doze-resp's and the two engines
//! must answer it alike; doze-resp itself has no engine to run it on.

use doze_module_proto::msg::blocks::{ChildrenArgs, TreeNode, VALUE_CHUNK, ValueArgs, media};
use doze_module_proto::msg::calls::RunAction;
use doze_module_proto::msg::error::ErrorKind;
use doze_module_proto::msg::instance::Address;
use doze_module_test::Booted;
use doze_resp::{Client, Reply};
use std::collections::BTreeMap;
use tokio::net::UnixStream;

/// A client logged in with `password`, straight to the backend.
pub(crate) async fn seed_client(b: &Booted, password: &str) -> Client<UnixStream> {
    let Some(Address::Unix(path)) = b.backend() else {
        panic!("a socket backend on Unix, got {:?}", b.backend())
    };
    Client::authenticated(UnixStream::connect(path).await.unwrap(), Some(password))
        .await
        .unwrap()
}

async fn ok(c: &mut Client<UnixStream>, args: &[&[u8]]) {
    let r = c.call_bytes(args).await.unwrap();
    assert!(!matches!(r, Reply::Error(_)), "{args:?}: {r:?}");
}

/// The keys the checks below look for.
async fn seed(c: &mut Client<UnixStream>, big: &[u8]) {
    for (k, v) in [
        (&b"a:b:c"[..], &b"1"[..]),
        (b"a:b:d", b"2"),
        (b"a", b"top"),
        (b"user", b"u"),
        (b"user:1", b"one"),
        (b"we*ird?", b"globby"),
        (b"weirdX", b"decoy"),
        (b"doc", br#"{"name":"Ada","tags":["x"]}"#),
        (b"note", "café au lait".as_bytes()),
        (b"bin", b"\x89PNG\x00\xff\xfe"),
    ] {
        ok(c, &[b"SET", k, v]).await;
    }
    ok(c, &[b"SET", b"big", big]).await;
    ok(c, &[b"HSET", b"h", b"f2", b"v2", b"f1", b"v1"]).await;
    ok(c, &[b"RPUSH", b"l", b"x", b"y", b"z"]).await;
    ok(c, &[b"SADD", b"s", b"m2", b"m1"]).await;
    ok(c, &[b"ZADD", b"z", b"2", b"two", b"1", b"one"]).await;
    ok(c, &[b"XADD", b"st", b"1-1", b"k", b"v"]).await;
    let many: Vec<Vec<u8>> = (0..1200)
        .map(|i| format!("many:{i:04}").into_bytes())
        .collect();
    let cmds: Vec<Vec<&[u8]>> = many.iter().map(|k| vec![&b"SET"[..], k, b"1"]).collect();
    for r in c.pipeline(&cmds).await.unwrap() {
        assert!(!matches!(r, Reply::Error(_)), "{r:?}");
    }
}

async fn children(
    b: &Booted,
    parent: &str,
    filter: &str,
    cursor: Option<String>,
) -> (Vec<TreeNode>, Option<String>) {
    let a = ChildrenArgs {
        instance: b.instance.clone(),
        panel: "keys".into(),
        parent: parent.into(),
        filter: filter.into(),
        cursor,
    };
    let page = b.module.children(&a).await.unwrap();
    (page.nodes, page.next)
}

/// Every page of a folder.
async fn all(b: &Booted, parent: &str, filter: &str) -> (Vec<TreeNode>, usize) {
    let (mut nodes, mut next) = children(b, parent, filter, None).await;
    let mut pages = 1;
    while let Some(c) = next {
        let (more, n) = children(b, parent, filter, Some(c)).await;
        nodes.extend(more);
        next = n;
        pages += 1;
    }
    (nodes, pages)
}

fn find<'a>(nodes: &'a [TreeNode], id: &str) -> &'a TreeNode {
    nodes.iter().find(|n| n.id == id).unwrap_or_else(|| {
        panic!(
            "no {id} in {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        )
    })
}

async fn value(
    b: &Booted,
    id: &str,
    offset: u64,
    max: u32,
) -> Result<doze_module_proto::msg::blocks::ValueAnswer, doze_module_proto::msg::error::Error> {
    let a = ValueArgs {
        instance: b.instance.clone(),
        panel: "keys".into(),
        row: BTreeMap::from([("id".to_string(), id.to_string())]),
        offset,
        max,
    };
    b.module.value(&a).await
}

/// Seeds the engine, then walks the tree, opens each kind of value, and sets keys with the form.
pub(crate) async fn browse_and_set(b: &Booted, password: &str) {
    let mut c = seed_client(b, password).await;
    // 1.5 MiB of text with a two-byte character straddling the first part's end.
    let mut big = vec![b'x'; VALUE_CHUNK as usize - 1];
    big.extend("é".as_bytes());
    big.extend(vec![b'y'; (VALUE_CHUNK / 2) as usize]);
    seed(&mut c, &big).await;

    levels(b).await;
    filter(b).await;
    big_string(b, &big).await;
    tree(b).await;
    values(b).await;
    form(b, &mut c).await;
    old_app(b).await;
}

/// The top level: folders with their counts, keys with their types.
async fn tree(b: &Booted) {
    // The top: folders with their key counts, keys with their types; a name that is both is both.
    let (top, _) = all(b, "", "").await;
    assert_eq!(
        find(&top, "f:a:").info.get("keys").map(String::as_str),
        Some("2")
    );
    assert!(!find(&top, "f:a:").leaf);
    assert!(find(&top, "k:a").leaf);
    assert_eq!(find(&top, "f:many:").info["keys"], "1200");
    assert_eq!(find(&top, "f:user:").label, "user");
    assert_eq!(find(&top, "k:user").label, "user");
    for (id, t) in [
        ("k:h", "hash"),
        ("k:l", "list"),
        ("k:s", "set"),
        ("k:z", "zset"),
        ("k:st", "stream"),
        ("k:bin", "string"),
    ] {
        assert_eq!(
            find(&top, id).info.get("type").map(String::as_str),
            Some(t),
            "{id}"
        );
    }
    assert!(
        !top.iter().any(|n| n.id.starts_with("k:many")),
        "keys under a folder stay in it"
    );
}

/// Down the tree a level at a time, and a folder too big for one page.
async fn levels(b: &Booted) {
    // Down a level at a time.
    let (a, _) = all(b, "f:a:", "").await;
    assert_eq!(
        a.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["f:a:b:"]
    );
    let (ab, _) = all(b, "f:a:b:", "").await;
    let ids: Vec<&str> = ab.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["k:a:b:c", "k:a:b:d"]);
    assert_eq!(ab[0].label, "c");
    // A folder of 1200 keys comes in pages.
    let (many, pages) = all(b, "f:many:", "").await;
    assert_eq!((many.len(), pages), (1200, 3));
    assert!(
        many.iter()
            .all(|n| n.leaf && n.info.get("type").map(String::as_str) == Some("string"))
    );
}

/// The filter.
async fn filter(b: &Booted) {
    // A filter searches every folder and names keys in full; glob characters in it match only themselves.
    let (hits, _) = all(b, "", "b:c").await;
    assert_eq!(
        hits.iter()
            .map(|n| (n.id.as_str(), n.label.as_str()))
            .collect::<Vec<_>>(),
        [("k:a:b:c", "a:b:c")]
    );
    let (hits, _) = all(b, "", "ird?").await;
    assert_eq!(
        hits.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["k:we*ird?"]
    );
}

/// Each kind of value, and a big one a part at a time.
async fn values(b: &Booted) {
    // Each kind of value.
    let v = value(b, "k:doc", 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::JSON, &br#"{"name":"Ada","tags":["x"]}"#[..])
    );
    let v = value(b, "k:note", 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::TEXT, "café au lait".as_bytes())
    );
    let v = value(b, "k:bin", 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::BYTES, &b"\x89PNG\x00\xff\xfe"[..]),
        "bytes, not lossy text"
    );
    let v = value(b, "k:h", 0, 0).await.unwrap();
    assert_eq!(v.media, media::PAIRS);
    assert_eq!(
        v.pairs,
        [
            ("f1".to_string(), "v1".to_string()),
            ("f2".into(), "v2".into())
        ]
    );
    assert!(v.body.is_empty());
    let v = value(b, "k:z", 0, 0).await.unwrap();
    assert_eq!(
        v.pairs,
        [
            ("one".to_string(), "1".to_string()),
            ("two".into(), "2".into())
        ]
    );
    let v = value(b, "k:l", 0, 0).await.unwrap();
    assert_eq!(
        (v.media.as_str(), v.body.as_slice()),
        (media::JSON, &br#"["x","y","z"]"#[..])
    );
    let v = value(b, "k:s", 0, 0).await.unwrap();
    assert_eq!(v.body, br#"["m1","m2"]"#.to_vec());
    let v = value(b, "k:st", 0, 0).await.unwrap();
    assert_eq!(
        v.body,
        br#"{"length":1,"entries":[{"id":"1-1","fields":{"k":"v"}}]}"#.to_vec()
    );
}

/// A big string, and keys that are not there.
async fn big_string(b: &Booted, big: &[u8]) {
    // A big string a part at a time: "Load the rest" asks from where the last part stopped.
    let first = value(b, "k:big", 0, VALUE_CHUNK).await.unwrap();
    assert_eq!(
        (first.media.as_str(), first.size, first.more),
        (media::TEXT, big.len() as u64, true)
    );
    assert_eq!(first.body.len(), VALUE_CHUNK as usize);
    let rest = value(b, "k:big", first.body.len() as u64, VALUE_CHUNK)
        .await
        .unwrap();
    assert!(!rest.more);
    assert_eq!([first.body, rest.body].concat(), big);
    // A key that is gone, and an id that is not a key.
    let e = value(b, "k:nope", 0, 0).await.unwrap_err();
    assert_eq!(
        (e.kind(), e.message.as_str()),
        (ErrorKind::NotFound, "no key nope")
    );
    assert_eq!(
        value(b, "f:a:", 0, 0).await.unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

/// An app that does not know forms, asking for the set-key form's action.
async fn old_app(b: &Booted) {
    // An app that does not know forms sends one box: with two required fields (key and value) nothing tells which the
    // box was, so it is told to update rather than have a guess written.
    let old = RunAction {
        instance: b.instance.clone(),
        action: "key-set".into(),
        resource: String::new(),
        input: Some("made:3 hello".into()),
        values: Default::default(),
        form: false,
    };
    let e = b.module.run_action(&old).await.unwrap_err();
    assert_eq!(e.message, doze_module::forms::NEEDS_NEWER_APP, "{e}");
}

/// The set-key form.
async fn form(b: &Booted, c: &mut Client<UnixStream>) {
    // The form sets a key, with a time to live when asked.
    let form = |values: &[(&str, &str)]| RunAction {
        instance: b.instance.clone(),
        action: "key-set".into(),
        resource: String::new(),
        input: None,
        values: values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        form: true,
    };
    let said = b
        .module
        .run_action(&form(&[("key", "made:1"), ("value", "hi there")]))
        .await
        .unwrap();
    assert_eq!(said, "set made:1");
    let said = b
        .module
        .run_action(&form(&[
            ("key", "made:2"),
            ("value", "brief"),
            ("ttl", "100"),
        ]))
        .await
        .unwrap();
    assert_eq!(said, "set made:2; it expires in 1m 40s");
    // The app sends every field, a blank time to live too: it keeps the key for ever.
    let said = b
        .module
        .run_action(&form(&[("key", "made:4"), ("value", "v"), ("ttl", "")]))
        .await
        .unwrap();
    assert_eq!(said, "set made:4");
    let get = c.call(&["GET", "made:1"]).await.unwrap();
    assert_eq!(get.text().as_deref(), Some("hi there"));
    let ttl = c.call(&["TTL", "made:2"]).await.unwrap();
    assert!(matches!(ttl, Reply::Integer(1..=100)), "{ttl:?}");
    assert_eq!(
        c.call(&["TTL", "made:1"]).await.unwrap(),
        Reply::Integer(-1),
        "0 keeps it forever"
    );
    let (made, _) = all(b, "f:made:", "").await;
    assert!(find(&made, "k:made:2").info.contains_key("ttl"));
    // An empty key is refused at its field, and so is a time to live out of range.
    let e = b
        .module
        .run_action(&form(&[("key", ""), ("value", "v")]))
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("key"), "{e}");
    let e = b
        .module
        .run_action(&form(&[("key", "k"), ("value", "v"), ("ttl", "-1")]))
        .await
        .unwrap_err();
    assert_eq!(e.field.as_deref(), Some("ttl"), "{e}");
}
