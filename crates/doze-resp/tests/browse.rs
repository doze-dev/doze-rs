//! The key tree against a stand-in server that answers `SCAN` a step at a time: pages follow their cursor, and a
//! keyspace too big to walk says so with a last node instead of walking it all.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use doze_resp::browse::{MAX_KEYS, children};
use doze_resp::{Client, Reply, parse};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

fn bulk(b: &[u8]) -> Vec<u8> {
    let mut out = format!("${}\r\n", b.len()).into_bytes();
    out.extend_from_slice(b);
    out.extend_from_slice(b"\r\n");
    out
}

/// Answers `SCAN` over `keys` a thousand at a time (the cursor is the index of the next), `TYPE` with `string` and
/// `TTL` with -1; counts the `SCAN` steps it was asked.
async fn serve(mut s: DuplexStream, keys: Vec<String>) {
    let mut buf = Vec::new();
    loop {
        let Some((cmd, used)) = parse(&buf).unwrap() else {
            let mut chunk = [0u8; 65536];
            let n = s.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            continue;
        };
        buf.drain(..used);
        let Reply::Array(Some(args)) = cmd else {
            panic!("{cmd:?}")
        };
        let arg = |i: usize| match &args[i] {
            Reply::Bulk(Some(b)) => String::from_utf8(b.clone()).unwrap(),
            other => panic!("{other:?}"),
        };
        let reply = match arg(0).as_str() {
            "SCAN" => {
                let from: usize = arg(1).parse().unwrap();
                let to = (from + 1000).min(keys.len());
                let next = if to == keys.len() { 0 } else { to };
                let mut r = b"*2\r\n".to_vec();
                r.extend(bulk(next.to_string().as_bytes()));
                r.extend(format!("*{}\r\n", to - from).into_bytes());
                for k in &keys[from..to] {
                    r.extend(bulk(k.as_bytes()));
                }
                r
            }
            "TYPE" => b"+string\r\n".to_vec(),
            "TTL" => b":-1\r\n".to_vec(),
            other => panic!("unexpected {other}"),
        };
        s.write_all(&reply).await.unwrap();
    }
}

fn client(keys: Vec<String>) -> Client<DuplexStream> {
    let (ours, theirs) = tokio::io::duplex(1 << 20);
    tokio::spawn(serve(theirs, keys));
    Client::new(ours)
}

#[tokio::test(flavor = "current_thread")]
async fn a_big_folder_comes_a_page_at_a_time() {
    let keys: Vec<String> = (0..1200).map(|i| format!("q:{i:04}")).collect();
    let mut c = client(keys);
    let top = children(&mut c, "", "", None).await.unwrap();
    assert_eq!(top.nodes.len(), 1);
    assert_eq!(
        (top.nodes[0].id.as_str(), top.nodes[0].info["keys"].as_str()),
        ("f:q:", "1200")
    );
    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let p = children(&mut c, "f:q:", "", cursor.as_deref())
            .await
            .unwrap();
        assert!(p.nodes.len() <= 500);
        assert!(p.nodes.iter().all(|n| n.leaf && n.info["type"] == "string"));
        seen.extend(p.nodes.into_iter().map(|n| n.label));
        pages += 1;
        match p.next {
            Some(n) => cursor = Some(n),
            None => break,
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 1200);
    assert_eq!((seen[0].as_str(), seen[1199].as_str()), ("0000", "1199"));
    // Ids this tree never gave have no children.
    assert!(
        children(&mut c, "k:q:0001", "", None)
            .await
            .unwrap()
            .nodes
            .is_empty()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_keyspace_too_big_to_walk_says_so() {
    let keys: Vec<String> = (0..MAX_KEYS + 5000)
        .map(|i| format!("big:{i:06}"))
        .collect();
    let mut c = client(keys);
    let top = children(&mut c, "", "", None).await.unwrap();
    let ids: Vec<&str> = top.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["f:big:", "m:"]);
    assert_eq!(
        top.nodes[0].info["keys"],
        MAX_KEYS.to_string(),
        "the count is of the keys looked at"
    );
    assert!(top.nodes[1].leaf);
    assert!(top.nodes[1].info["note"].contains("filter"));
}
